use alloy::consensus::{
    Eip658Value, EthereumTxEnvelope, Receipt, ReceiptEnvelope, ReceiptWithBloom, TxEip4844,
    TxEip4844Variant,
};
use alloy::primitives::{Address, Bloom, Bytes, B256, U256};
use alloy::rlp::Decodable;
use alloy::rpc::types::{
    Block, BlockId, BlockNumberOrTag, Filter, Log, Transaction, TransactionReceipt,
    TransactionRequest,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use golem_base_sdk::entity::Entity;
use golem_base_sdk::rpc::{EntityMetaData, SearchResult};
use jsonrpsee::core::{async_trait, RpcResult, StringError, SubscriptionResult};
use jsonrpsee::types::{ErrorCode, ErrorObject};
use jsonrpsee::{PendingSubscriptionSink, SubscriptionMessage};
use std::sync::Arc;

use crate::api::{EthRpcServer, GolemBaseRpcServer};
use crate::block::Transaction as InternalTransaction;
use crate::blockchain::Blockchain;
use crate::controller::{should_fail, CallOverride, CallResponse, MockController, WithCallback};
use crate::entity_db::EntityDb;
use crate::events::EventEmitter;
use crate::execution::ExecutionEngine;
use crate::managed_accounts::ManagedAccounts;
use crate::transaction_pool::TransactionPool;

pub mod api;
pub mod block;
pub mod block_builder;
pub mod blockchain;
pub mod controller;
pub mod display;
pub mod entity_db;
pub mod events;
pub mod execution;
pub mod managed_accounts;
pub mod query_parser;
pub mod server;
pub mod transaction_pool;

// Re-export symbols for user of the library.
pub use server::GolemBaseMockServer;

/// Helper function to create ErrorObject with a typed ErrorCode and message
pub fn create_error(code: ErrorCode, message: impl Into<String>) -> ErrorObject<'static> {
    ErrorObject::owned(code.code(), message.into(), None::<()>)
}

pub fn invalid_param(message: impl Into<String>) -> ErrorObject<'static> {
    create_error(ErrorCode::InvalidParams, message)
}

/// Macro to handle RPC overrides with custom response support.
///
/// This macro handles the CallResponse::Custom variant by extracting the response
/// and returning it immediately, while keeping the override reference alive for other cases.
#[macro_export]
macro_rules! return_override {
    ($override:ident, $return_type:ty) => {{
        use jsonrpsee::core::RpcResult;
        use jsonrpsee::types::ErrorCode;
        use $crate::controller::{CallOverride, CallResponse};

        if let Some(override_wrapper) = &$override {
            let response = match &override_wrapper.response {
                CallOverride::Once(resp) => resp,
                CallOverride::Until { response: resp, .. } => resp,
                CallOverride::NTimes { response: resp, .. } => resp,
                CallOverride::Always(resp) => resp,
            };

            if let CallResponse::Custom(json_obj) = response {
                // For custom responses, we need to decode and return the value
                match json_obj.decode::<$return_type>() {
                    Ok(value) => return RpcResult::Ok(value),
                    Err(e) => {
                        return RpcResult::Err($crate::create_error(
                            ErrorCode::InternalError,
                            format!("Failed to decode custom response: {e}"),
                        ))
                    }
                }
            }
        }
    }};
}

/// Mock implementation of RPC methods (both Ethereum and GolemBase)
#[derive(Clone)]
pub struct GolemBaseMock {
    blockchain: Blockchain,
    entity_db: EntityDb,
    transaction_pool: TransactionPool,
    execution: ExecutionEngine,
    managed_accounts: ManagedAccounts,
    controller: MockController,
    event_emitter: Arc<EventEmitter>,
}

impl GolemBaseMock {
    pub fn new() -> Self {
        let entity_db = EntityDb::new();
        let events = Arc::new(EventEmitter::new());
        let chain_id = 1337u64;
        let blockchain = Blockchain::new(entity_db.clone(), events.clone(), chain_id);
        let transaction_pool = TransactionPool::new();
        let managed_accounts = ManagedAccounts::new();
        let execution_engine = ExecutionEngine::new(
            blockchain.clone(),
            transaction_pool.clone(),
            managed_accounts.clone(),
        );

        Self {
            blockchain,
            entity_db,
            transaction_pool,
            execution: execution_engine,
            managed_accounts,
            controller: MockController::new(),
            event_emitter: events,
        }
    }

    pub fn set_chain_id(&self, chain_id: u64) {
        self.blockchain.set_chain_id(chain_id);
    }

    /// Finds the next override for the given RPC name.
    /// If override is expected to return error, it will be immediately returned.
    /// Otherwise, the struct will be returned that will notify client on drop
    ///
    /// Note that this function must be used in very specific way to work correctly:
    /// ```
    /// # use golem_base_mock::{GolemBaseMock, return_override};
    /// # use jsonrpsee::core::RpcResult;
    ///
    /// fn main() -> RpcResult<()> {
    ///     let mock = GolemBaseMock::new();
    ///
    ///     let _override = mock.next_override("eth_getTransactionCount")?;
    ///     return_override!(_override, ());
    ///
    ///     RpcResult::Ok(())
    /// }
    /// ```
    /// Override must not be dropped before going out of the scope of RPC call.
    /// Otherwise, it will send a notification to the client, before we will finish processing the call.
    ///
    /// On the other side, caller should never handle the error, but he should return immediately.
    /// Otherwise the notification will be sent too early.
    #[must_use]
    pub fn next_override(
        &self,
        rpc_name: &str,
    ) -> Result<Option<WithCallback<CallOverride>>, ErrorObject<'static>> {
        match self.controller.take_next_override(rpc_name) {
            Some(override_response) => match override_response.response().clone() {
                // We found override that will return error, so we return it immediately.
                // WithCallback struct will be dropped and send the notification.
                // It will happen already in this function, so we expect that caller, won't do
                // anything complicated afterwards.
                CallResponse::Error(err) => {
                    return Err(create_error(ErrorCode::InternalError, err.to_string()));
                }
                // FailEachNth response: return error based on frequency fraction
                CallResponse::FailEachNth { error, frequency } => {
                    if should_fail(frequency, override_response.call_count) {
                        return Err(create_error(ErrorCode::InternalError, error));
                    }
                    Ok(Some(override_response))
                }
                // Caller should process normal logic, but we need to notify the client.
                // We return WithCallback struct that will do this on drop.
                CallResponse::Success => Ok(Some(override_response)),
                // We return the custom response as is and regular logic won't be executed.
                CallResponse::Custom(_) => Ok(Some(override_response)),
            },
            // No override found, so we don't need to do anything special. Caller
            // will just process the normal logic.
            None => Ok(None),
        }
    }
}

#[async_trait]
impl EthRpcServer for GolemBaseMock {
    async fn get_transaction_count(
        &self,
        address: Address,
        block: Option<BlockId>,
    ) -> RpcResult<U256> {
        let _override = self.next_override("eth_getTransactionCount")?;
        return_override!(_override, U256);

        log::debug!("Getting transaction count for address {address} with block {block:?}");

        // Get pending transactions from the pool
        let pending_count = self.transaction_pool.get_transaction_count(&address).await;

        // Get the account nonce (already processed transactions) from the blockchain
        let account_nonce = if let Some(account) = self.blockchain.get_account(&address).await {
            account.nonce
        } else {
            U256::ZERO
        };

        let total_count = match block {
            Some(BlockId::Number(BlockNumberOrTag::Latest)) => account_nonce,
            Some(BlockId::Number(BlockNumberOrTag::Safe)) => account_nonce,
            Some(BlockId::Number(BlockNumberOrTag::Finalized)) => account_nonce,
            Some(BlockId::Number(BlockNumberOrTag::Pending)) => account_nonce + pending_count,
            _ => {
                return Err(create_error(
                    ErrorCode::InternalError,
                    format!("Invalid block ID: {block:?}"),
                ))
            }
        };
        Ok(total_count)
    }

    async fn get_transaction_receipt(&self, hash: B256) -> RpcResult<Option<TransactionReceipt>> {
        log::debug!("Getting transaction receipt for hash: 0x{:x}", hash);
        let _override = self.next_override("eth_getTransactionReceipt")?;

        // Get transaction from blockchain first, then from pool if not found
        let transaction = if let Some(tx) = self.blockchain.get_transaction(&hash).await {
            log::debug!(
                "Transaction found in blockchain: from={:?}, to={:?}",
                tx.from,
                tx.to
            );
            tx
        } else {
            log::debug!("Transaction not found anywhere");
            return Ok(None);
        };

        // Try to find the block containing this transaction (may be None for pending transactions)
        let block = match self
            .blockchain
            .find_block_containing_transaction(&hash)
            .await
        {
            Some(block) => {
                log::debug!(
                    "Transaction found in block: number={}, hash=0x{:x}",
                    block.header.block_number,
                    block.header.block_hash
                );
                block
            }
            None => {
                log::debug!("Transaction not in any block (pending)");
                return Ok(None);
            }
        };

        let logs: Vec<Log> = block
            .get_all_logs()
            .iter()
            .enumerate()
            .map(|(log_index, log)| Log {
                block_timestamp: Some(block.header.timestamp),
                block_hash: Some(block.header.block_hash),
                block_number: Some(block.header.block_number),
                transaction_hash: Some(hash),
                transaction_index: block.find_transaction_index(&hash),
                log_index: Some(log_index as u64),
                removed: false,
                inner: log.to_log_data(),
            })
            .collect();

        let receipt = TransactionReceipt {
            transaction_hash: hash,
            transaction_index: block.find_transaction_index(&hash),
            block_hash: Some(block.header.block_hash),
            block_number: Some(block.header.block_number),
            from: transaction.from,
            to: Some(transaction.to),
            inner: ReceiptEnvelope::Eip1559(ReceiptWithBloom {
                receipt: Receipt {
                    status: Eip658Value::success(),
                    cumulative_gas_used: 0,
                    logs: logs,
                },
                logs_bloom: Bloom::ZERO,
            }),
            gas_used: 0,
            effective_gas_price: 0,
            blob_gas_used: None,    // No blob gas in this mock
            blob_gas_price: None,   // No blob gas in this mock
            contract_address: None, // No contract creation in this mock
        };
        Ok(Some(receipt))
    }

    async fn get_proof(
        &self,
        _address: Address,
        _keys: Vec<B256>,
        _block: Option<BlockId>,
    ) -> RpcResult<serde_json::Value> {
        // Mock implementation - return empty proof
        Ok(serde_json::json!({
            "address": "0x0000000000000000000000000000000000000000",
            "accountProof": [],
            "balance": "0x0",
            "codeHash": "0x0000000000000000000000000000000000000000000000000000000000000000",
            "nonce": "0x0",
            "storageHash": "0x0000000000000000000000000000000000000000000000000000000000000000",
            "storageProof": []
        }))
    }

    async fn get_balance(&self, address: Address, _block: Option<BlockId>) -> RpcResult<U256> {
        let _override = self.next_override("eth_getBalance")?;
        return_override!(_override, U256);
        Ok(self.blockchain.get_balance(&address).await)
    }

    async fn accounts(&self) -> RpcResult<Vec<Address>> {
        let _override = self.next_override("eth_accounts")?;
        return_override!(_override, Vec<Address>);
        // Return list of managed accounts
        Ok(self.managed_accounts.get_all_accounts())
    }

    async fn get_accounts(&self) -> RpcResult<Vec<Address>> {
        let _override = self.next_override("golem_getAccounts")?;
        return_override!(_override, Vec<Address>);
        Ok(self.blockchain.get_accounts().await)
    }

    async fn send_transaction(&self, transaction: TransactionRequest) -> RpcResult<B256> {
        let _override = self.next_override("eth_sendTransaction")?;

        // Log the transaction data
        log::info!(
            "Received transaction: {}",
            serde_json::to_string_pretty(&transaction)
                .unwrap_or_else(|_| "Invalid JSON".to_string())
        );

        // Validate chain ID - it's required
        let tx_chain_id = transaction
            .chain_id
            .ok_or_else(|| invalid_param("Missing chain ID in transaction"))?;

        self.blockchain
            .validate_chain_id(tx_chain_id)
            .map_err(|e| invalid_param(e.to_string()))?;

        // Get the sender address
        let from_address = transaction
            .from
            .ok_or_else(|| invalid_param("Missing 'from' field in transaction".to_string()))?;

        // Get the account for the sender address
        let signer = self
            .managed_accounts
            .get_account(from_address)
            .ok_or_else(|| {
                invalid_param(format!(
                    "Account {from_address} is not managed by this node.",
                ))
            })?;

        let nonce = match transaction.nonce {
            Some(tx_nonce) => tx_nonce,
            None => {
                // Get current nonce from blockchain if not specified
                self.blockchain
                    .get_nonce(&from_address)
                    .await
                    .try_into()
                    .unwrap_or(0)
            }
        };
        let transaction = transaction.nonce(nonce);

        let (signature, signed) = InternalTransaction::sign_request(transaction.clone(), &signer)
            .await
            .map_err(|e| invalid_param(format!("Error signing transaction: {e}")))?;

        let transaction = Arc::new(
            InternalTransaction::from_signed(transaction, &signature, &signed).map_err(|e| {
                invalid_param(format!(
                    "Error converting to internal transaction representation: {e}"
                ))
            })?,
        );

        // Validate transaction nonce before adding to pool
        self.blockchain
            .validate_transaction_nonce(&transaction)
            .await
            .map_err(|e| invalid_param(e.to_string()))?;

        self.transaction_pool
            .add_transaction(transaction.clone())
            .await;

        log::info!(
            "Added transaction to pool with hash: 0x{:x}",
            transaction.hash
        );
        Ok(transaction.hash)
    }

    async fn send_raw_transaction(&self, data: Bytes) -> RpcResult<B256> {
        let _override = self.next_override("eth_sendRawTransaction")?;

        // Use the bytes directly since input is already Bytes
        let tx_bytes = data.to_vec();

        // Decode the RLP-encoded transaction
        let decoded = EthereumTxEnvelope::<TxEip4844>::decode(&mut &tx_bytes[..]).map_err(|e| {
            create_error(
                ErrorCode::ParseError,
                format!("Failed to decode transaction: {e}"),
            )
        })?;

        // Convert decoded transaction to our internal Transaction type
        let transaction = crate::block::Transaction::try_from(decoded).map_err(|e| {
            create_error(
                ErrorCode::InvalidParams,
                format!("Failed to convert transaction: {e}"),
            )
        })?;

        // Validate chain ID - it's required
        self.blockchain
            .validate_chain_id(transaction.chain_id)
            .map_err(|e| invalid_param(e.to_string()))?;

        let transaction = Arc::new(transaction);

        // Validate transaction nonce before adding to pool
        self.blockchain
            .validate_transaction_nonce(&transaction)
            .await
            .map_err(|e| create_error(ErrorCode::InvalidParams, e.to_string()))?;

        self.transaction_pool
            .add_transaction(transaction.clone())
            .await;

        Ok(transaction.hash)
    }

    async fn chain_id(&self) -> RpcResult<U256> {
        let _override = self.next_override("eth_chainId")?;
        return_override!(_override, U256);
        Ok(U256::from(self.blockchain.chain_id()))
    }

    async fn get_transaction_by_hash(&self, hash: B256) -> RpcResult<Option<Transaction>> {
        let _override = self.next_override("eth_getTransactionByHash")?;

        // First check the blockchain for mined transactions (to get block info)
        if let Some(tx) = self.blockchain.get_transaction(&hash).await {
            // Since block is added to chain, we need to find in which exact block it is.
            let block = self
                .blockchain
                .find_block_containing_transaction(&hash)
                .await
                .ok_or(create_error(
                    ErrorCode::InternalError,
                    format!("Failed to find block containing transaction: {hash}"),
                ))?;
            let idx = block.find_transaction_index(&hash).ok_or(create_error(
                ErrorCode::InternalError,
                format!("Failed to find transaction index in block: {hash}"),
            ))?;

            // Build Transaction from block and transaction data
            let tx_envelope: EthereumTxEnvelope<TxEip4844Variant> = tx.to_envelope();
            let alloy_tx = Transaction {
                inner: tx_envelope.try_into_recovered().map_err(|e| {
                    create_error(
                        ErrorCode::InternalError,
                        format!("Failed to convert transaction: {e}"),
                    )
                })?,
                block_hash: Some(block.header.block_hash),
                block_number: Some(block.header.block_number),
                transaction_index: Some(idx),
                effective_gas_price: None,
            };
            return Ok(Some(alloy_tx));
        }

        // If not found in blockchain, check the transaction pool for pending transactions
        if let Some(tx) = self.transaction_pool.get_transaction(&hash).await {
            // Convert our internal Transaction to alloy Transaction format for pending transactions
            let tx_envelope: EthereumTxEnvelope<TxEip4844Variant> = tx.to_envelope();
            let alloy_tx = Transaction {
                inner: tx_envelope.try_into_recovered().map_err(|e| {
                    create_error(
                        ErrorCode::InternalError,
                        format!("Failed to convert pending transaction: {e}"),
                    )
                })?,
                block_hash: None,
                block_number: None,
                transaction_index: None,
                effective_gas_price: None,
            };
            return Ok(Some(alloy_tx));
        }

        // Transaction not found in pool or blockchain
        Ok(None)
    }

    async fn syncing(&self) -> RpcResult<bool> {
        let _override = self.next_override("eth_syncing")?;
        return_override!(_override, bool);
        Ok(false) // Mock implementation - always false
    }

    async fn get_block_by_number(
        &self,
        block: BlockNumberOrTag,
        _full: Option<bool>,
    ) -> RpcResult<Option<Block>> {
        // Get block number from BlockNumberOrTag
        let block_number = match block {
            BlockNumberOrTag::Number(num) => num,
            BlockNumberOrTag::Latest | BlockNumberOrTag::Safe | BlockNumberOrTag::Finalized => {
                // For mock, just return the latest block number
                match self.blockchain.get_latest_block_number().await {
                    Ok(num) => num,
                    Err(_) => return Ok(None), // Return None if no blocks exist
                }
            }
            BlockNumberOrTag::Earliest => 0,
            BlockNumberOrTag::Pending => {
                // Return None for pending blocks in mock implementation
                return Ok(None);
            }
        };

        let block = self.blockchain.get_block_by_number(block_number).await;
        Ok(block.map(|block| (*block).clone().into()))
    }

    async fn estimate_gas(&self, _call_request: serde_json::Value) -> RpcResult<U256> {
        let _override = self.next_override("eth_estimateGas")?;
        return_override!(_override, U256);
        // Mock implementation - return a reasonable gas estimate
        // In a real implementation, this would simulate the transaction and estimate gas
        Ok(U256::from(21000))
    }

    async fn fee_history(
        &self,
        _block_count: U256,
        _newest_block: BlockId,
        _reward_percentiles: Option<Vec<f64>>,
    ) -> RpcResult<serde_json::Value> {
        let _override = self.next_override("eth_feeHistory")?;
        return_override!(_override, serde_json::Value);
        // Mock implementation - return empty fee history
        // In a real implementation, this would return actual fee history data
        Ok(serde_json::json!({
            "oldestBlock": "0x0",
            "baseFeePerGas": [],
            "gasUsedRatio": [],
            "reward": []
        }))
    }

    async fn gas_price(&self) -> RpcResult<U256> {
        let _override = self.next_override("eth_gasPrice")?;
        return_override!(_override, U256);
        // Mock implementation - return a reasonable gas price
        // In a real implementation, this would return the current gas price from the network
        Ok(U256::from(20_000_000_000u64)) // 20 gwei
    }

    async fn block_number(&self) -> RpcResult<U256> {
        let _override = self.next_override("eth_blockNumber")?;
        return_override!(_override, U256);
        match self.blockchain.get_latest_block_number().await {
            Ok(num) => Ok(U256::from(num)),
            Err(e) => Err(create_error(
                ErrorCode::InternalError,
                format!("Error getting block: {e}"),
            )),
        }
    }

    async fn subscribe(
        &self,
        subscription_sink: PendingSubscriptionSink,
        subscription_type: String,
        filter: Option<Filter>,
    ) -> SubscriptionResult {
        if subscription_type == "logs" {
            // Subscribe to the event emitter
            let mut event_receiver = self
                .event_emitter
                .subscribe_to_events(filter.clone())
                .await?;

            // Accept the subscription and forward events to the sink
            let sink = subscription_sink.accept().await?;

            // Spawn a task to forward events from the receiver to the sink
            tokio::spawn(async move {
                while let Ok(log_event) = event_receiver.recv().await {
                    let log: Log = log_event.into();
                    if let Ok(msg) = SubscriptionMessage::from_json(&log) {
                        sink.send(msg).await.ok();
                    }
                }
            });

            Ok(())
        } else {
            Err(StringError::from(format!(
                "Unsupported subscription type: {subscription_type}",
            )))
        }
    }
}

#[async_trait]
impl GolemBaseRpcServer for GolemBaseMock {
    async fn get_entity(&self, key: B256) -> RpcResult<Option<Entity>> {
        let _override = self.next_override("golem_getEntity")?;
        return_override!(_override, Option<Entity>);

        Ok(self
            .entity_db
            .get_entity(&key)
            .await
            .map(|local_entity| {
                local_entity.try_into().map_err(|e| {
                    create_error(
                        ErrorCode::InternalError,
                        format!("Failed to convert entity data to UTF-8: {}", e),
                    )
                })
            })
            .transpose()?)
    }

    async fn get_entity_metadata(&self, key: B256) -> RpcResult<EntityMetaData> {
        let _override = self.next_override("golem_getEntityMetadata")?;
        return_override!(_override, EntityMetaData);

        Ok(self
            .entity_db
            .get_entity(&key)
            .await
            .map(|entity| EntityMetaData::from(&entity))
            .ok_or_else(|| {
                create_error(ErrorCode::InvalidParams, format!("entity {key} not found"))
            })?)
    }

    async fn get_entity_count(&self) -> RpcResult<u64> {
        let _override = self.next_override("golem_getEntityCount")?;
        return_override!(_override, u64);
        Ok(self.entity_db.count().await as u64)
    }

    async fn get_all_entity_keys(&self) -> RpcResult<Option<Vec<B256>>> {
        let _override = self.next_override("golem_getAllEntityKeys")?;
        return_override!(_override, Option<Vec<B256>>);
        Ok(Some(self.entity_db.get_all_keys().await))
    }

    async fn get_entities_of_owner(&self, address: Address) -> RpcResult<Option<Vec<B256>>> {
        let _override = self.next_override("golem_getEntitiesOfOwner")?;
        return_override!(_override, Option<Vec<B256>>);
        // Use the owner index to efficiently get entities by owner
        let keys = self.entity_db.get_entities_by_owner(&address).await;
        Ok(Some(keys))
    }

    async fn get_storage_value(&self, key: B256) -> RpcResult<String> {
        let _override = self.next_override("golem_getStorageValue")?;
        return_override!(_override, String);
        if let Some(entity) = self.entity_db.get_entity(&key).await {
            let encoded = BASE64.encode(&entity.data);
            Ok(encoded)
        } else {
            Err(create_error(
                ErrorCode::InvalidParams,
                format!("Entity not found for key: 0x{:x}", key),
            ))
        }
    }

    async fn query_entities(&self, query: String) -> RpcResult<Vec<SearchResult>> {
        let _override = self.next_override("golem_queryEntities")?;
        return_override!(_override, Vec<SearchResult>);
        let entities = self.entity_db.query_entities(&query).await.map_err(|e| {
            create_error(
                ErrorCode::InvalidParams,
                format!("Query parsing failed: {}", e),
            )
        })?;

        let results: Vec<SearchResult> = entities
            .into_iter()
            .map(|entity| SearchResult {
                key: entity.key,
                value: entity.data,
            })
            .collect();
        Ok(results)
    }

    async fn get_entities_to_expire_at_block(
        &self,
        _block_number: u64,
    ) -> RpcResult<Option<Vec<B256>>> {
        let _override = self.next_override("golem_getEntitiesToExpireAtBlock")?;
        return_override!(_override, Option<Vec<B256>>);
        // For now, return empty list since the EntityDb doesn't track expiration blocks
        // In a real implementation, you'd want to add an expiration index to the EntityDb
        Ok(Some(vec![]))
    }
}

impl GolemBaseMock {
    /// Creates a new account with a random private key
    pub fn create_account(&self) -> Address {
        self.managed_accounts.create_account()
    }
}
