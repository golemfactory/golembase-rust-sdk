use alloy::consensus::transaction::SignerRecoverable;
use alloy::consensus::{
    EthereumTxEnvelope, EthereumTypedTransaction, SignableTransaction, Signed, TxEip4844,
    TxEip4844Variant,
};
use alloy::hex;
use alloy::network::TransactionBuilder;
use alloy::primitives::{address, keccak256, Address, B256, U256};
use alloy::providers::PendingTransactionConfig;
use alloy::rpc::types::eth::TransactionRequest;
use alloy::rpc::types::TransactionReceipt;
use alloy_rlp::{Decodable, Encodable};
use anyhow::{anyhow, bail, Result};
use bigdecimal::BigDecimal;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::runtime::Builder;
use tokio::sync::{mpsc, oneshot};
use tokio::task::LocalSet;

use crate::client::TransactionConfig;
use crate::entity::{ArkivTransaction, Hash};
use crate::resilient_provider::ResilientProvider;
use crate::signers::TransactionSigner;
use crate::utils::eth_to_wei;

/// Helper function to display an Option value
fn display_option<T: std::fmt::Display>(opt: &Option<T>) -> String {
    opt.as_ref()
        .map(|n| n.to_string())
        .unwrap_or_else(|| "None".to_string())
}

/// Contains all three nonce values for an account
#[derive(Debug, Clone, derive_more::Display)]
#[display("Last tracked nonce: {}, next pending nonce: {next_pending_nonce}, current account nonce: {account_nonce}", display_option(&last_used_nonce))]
pub struct NonceInfo {
    /// Last nonce used by SDK code (saved during previous call to process_transaction)
    pub last_used_nonce: Option<u64>,
    /// Next nonce including pending transactions
    pub next_pending_nonce: u64,
    /// Current nonce value from blockchain (represents next nonce after last confirmed transaction)
    pub account_nonce: u64,
}

impl NonceInfo {
    /// Picks the appropriate nonce for the next transaction and logs relevant information.
    /// Returns the maximum of next_pending_nonce and (last_used_nonce + 1).
    pub fn pick_nonce(&self) -> u64 {
        log::info!("Nonce info: {self}");

        let nonce = match self.last_used_nonce {
            Some(last_used) => std::cmp::max(self.next_pending_nonce, last_used + 1),
            None => self.next_pending_nonce,
        };

        let pending = self.pending_transactions_count();
        if pending > 0 {
            log::debug!("Still processing {pending} pending transactions");
        }

        if let Some(last_used) = self.last_used_nonce {
            if (last_used + 1) < self.next_pending_nonce {
                log::warn!("Last used nonce is not equal to next pending nonce. Probably transaction was sent externally.");
            } else if (last_used + 1) > self.next_pending_nonce {
                if self.next_pending_nonce == 0 {
                    // If DB-chain was re-deployed, we need to reset nonce to 0.
                    log::warn!(
                        "Last used nonce was reset to 0. Probably DB-chain was re-deployed."
                    );
                    return 0;
                } else {
                    log::error!("Next pending nonce is less than last used nonce, but not 0. This should not happen!");
                }
            }
        }

        nonce
    }

    /// Returns the number of pending transactions for this account.
    /// Returns the count of transactions that have been sent but not yet mined.
    pub fn pending_transactions_count(&self) -> u64 {
        let pending = self.next_pending_nonce as i64 - self.account_nonce as i64;
        pending.max(0) as u64
    }
}

/// The address of the Arkiv storage processor contract.
/// All storage-related transactions are sent to this contract address.
pub const ARKIV_STORAGE_PROCESSOR_ADDRESS: Address =
    address!("0x0000000000000000000000000000000060138453");

/// Response type for queued transactions.
/// Used internally for passing transaction results through channels.
type TransactionResponse = Result<TransactionReceipt>;

/// Channel for transaction response.
/// Allows awaiting the result of a queued transaction asynchronously.
pub struct TransactionChannel {
    response_rx: oneshot::Receiver<TransactionResponse>,
}

impl TransactionChannel {
    /// Awaits the transaction receipt from the queue worker.
    /// Returns the transaction receipt or an error if the channel is closed.
    pub async fn receipt(self) -> Result<TransactionReceipt> {
        self.response_rx
            .await
            .map_err(|e| anyhow!("Failed to get transaction response: {}", e))?
    }
}

/// Message type for the transaction queue.
/// Contains the transaction request and a channel to send the result back.
struct QueueMessage {
    request: TransactionRequest,
    response_tx: oneshot::Sender<TransactionResponse>,
}

/// Queue for managing transaction submissions.
/// Handles signing, sending, and awaiting receipts for transactions in a background worker.
struct TransactionQueue {
    sender: mpsc::Sender<QueueMessage>,
    signer: Arc<Box<dyn TransactionSigner>>,
    provider: ResilientProvider,
    tx_config: Arc<TransactionConfig>,
    /// Last nonce used by SDK code (saved during previous call to process_transaction)
    last_used_nonce: Mutex<Option<u64>>,
}

/// Event signature for extending BTL (block time to live) of an entity.
/// Used to identify `ArkivStorageEntityBTLExtended` events in logs.
pub fn arkiv_storage_entity_btl_extended() -> B256 {
    keccak256(b"ArkivStorageEntityBTLExtended(uint256,uint256)")
}

impl TransactionQueue {
    /// Creates a new transaction queue and spawns a worker task to process transactions.
    /// The worker signs, sends, and tracks receipts for all queued transactions.
    fn new(
        provider: ResilientProvider,
        signer: Arc<Box<dyn TransactionSigner>>,
        tx_config: Arc<TransactionConfig>,
    ) -> Arc<Self> {
        let (tx, rx) = mpsc::channel(32);
        let queue = Arc::new(Self {
            sender: tx,
            signer,
            provider,
            tx_config,
            last_used_nonce: Mutex::new(None),
        });
        Self::spawn_worker(rx, queue.clone());
        queue
    }

    /// Signs a transaction request using the account's signer.
    /// Returns the signed transaction ready for encoding and submission.
    async fn sign_transaction(
        &self,
        tx: TransactionRequest,
    ) -> anyhow::Result<Signed<EthereumTypedTransaction<TxEip4844Variant>>> {
        let tx = tx.build_unsigned()?;
        let bytes = tx.encoded_for_signing();

        let signature = self.signer.sign(&bytes).await?;
        Ok(tx.into_signed(signature))
    }

    /// Encodes a signed transaction to RLP bytes for network submission.
    /// Also logs the transaction hash and attempts to decode and recover the signer for debugging.
    fn encode_transaction(
        &self,
        signed: &Signed<EthereumTypedTransaction<TxEip4844Variant>>,
    ) -> anyhow::Result<Vec<u8>> {
        let mut encoded = Vec::new();
        signed.eip2718_encode(&mut encoded);

        log::trace!(
            "RLP encoded transaction (hash: 0x{:x}): 0x{}",
            signed.hash(),
            hex::encode(&encoded)
        );

        // Decode the transaction for debugging purposes.
        let decoded_tx = EthereumTxEnvelope::<TxEip4844>::decode(&mut &encoded[..])
            .map_err(|e| anyhow!("Failed to decode transaction: {e}"))?;
        log::debug!("Decoded transaction: {:#?}", decoded_tx);

        let signer = decoded_tx
            .recover_signer()
            .map_err(|e| anyhow!("Failed to recover signer: {e}"))?;
        log::debug!("Recovered signer: {:#?}", signer);

        Ok(encoded)
    }

    /// Gets a transaction receipt with retries for "transaction indexing is in progress" errors.
    /// Waits until the transaction is indexed and the receipt is available, or returns None if timeout.
    async fn get_receipt_with_retry(
        &self,
        tx_hash: Hash,
    ) -> anyhow::Result<Option<TransactionReceipt>> {
        let timeout = self.tx_config.transaction_receipt_timeout.clone();
        get_receipt(
            &self.provider,
            tx_hash,
            Some(timeout),
            self.tx_config.required_confirmations,
        )
        .await
    }

    /// Returns a new TransactionRequest with bumped tip and fee cap by a percentage for replacement transactions.
    #[allow(unused)]
    fn bump_fees(&self, request: &TransactionRequest, attempt: u32) -> TransactionRequest {
        let bump_percent = self.tx_config.price_bump_percent * attempt as u128;
        let tip = request
            .max_priority_fee_per_gas
            .unwrap_or(self.tx_config.max_priority_fee_per_gas);
        let fee_cap = request
            .max_fee_per_gas
            .unwrap_or(self.tx_config.max_fee_per_gas);
        let bumped_tip = tip + (tip * bump_percent).div_ceil(100);
        let bumped_fee_cap = fee_cap + (fee_cap * bump_percent).div_ceil(100);
        request
            .clone()
            .with_max_priority_fee_per_gas(bumped_tip)
            .with_max_fee_per_gas(bumped_fee_cap)
    }

    /// Processes a single transaction:
    /// - Gets the current nonce for the sender.
    /// - Signs and encodes the transaction.
    /// - Sends the transaction and waits for it to be mined.
    /// - Returns the transaction receipt.
    async fn process_transaction(&self, request: TransactionRequest) -> TransactionResponse {
        // Get the current nonce for the sender address.
        let from = request
            .from
            .ok_or_else(|| anyhow!("Transaction request missing 'from' address"))?;

        // Wait for any pending transactions to be mined before proceeding.
        // We allow only a single transaction to be pending at a time.
        // If something goes wrong on blockchain and transaction can't pass, in most cases
        // there is nothing we can do about it. We should avoid sending new transactions, because
        // there will be more transactions stacked in the mempool.
        // We shouldn't rather replace transactions, because bumping gas price introduces another
        // layer of complexity.
        self.wait_for_pending_transactions(from, self.tx_config.pending_transaction_timeout)
            .await?;

        // We have 2 sources of nonces: our last used nonce and RPC.
        // RPC returns nonce of last pending transaction and nonce associated with the account.
        // Since we have no guarantee of sending requests to the same RPC, we can't trust fully
        // that it have full knowledge the returned nonces. At the same time tools outside of our
        // control can send transactions as well.
        let nonce_info = self.get_nonces(from).await?;
        let nonce = nonce_info.pick_nonce();

        // Update the request with the next pending nonce.
        let mut request = request.with_nonce(nonce);

        let max_retries = self.tx_config.max_retries;
        let mut attempt: u32 = 0;

        loop {
            // Sign and encode the transaction.
            let signed = self.sign_transaction(request.clone()).await?;
            let encoded = self.encode_transaction(&signed)?;

            attempt += 1;

            let pending = match self.provider.send_raw_transaction(&encoded).await {
                Ok(pending) => pending,
                // Retry transaction with updated nonce.
                Err(e) if e.to_string().contains("nonce too low") => {
                    let nonce_info = self.get_nonces(from).await?;
                    let nonce = nonce_info.pick_nonce();
                    request.set_nonce(nonce);
                    continue;
                }
                Err(e) => return Err(anyhow!("Failed to send transaction: {e}")),
            };

            let tx_hash = *pending.tx_hash();

            log::debug!("Transaction attempt {attempt} sent with hash: {tx_hash}");

            if let Some(receipt) = self.get_receipt_with_retry(tx_hash).await? {
                log::info!("Transaction succeeded on attempt {attempt} with hash: {tx_hash}");

                self.set_last_used_nonce(nonce);
                return Ok(receipt);
            }

            if attempt >= max_retries {
                return Err(anyhow!(
                    "Transaction failed after {max_retries} attempts, last hash: {tx_hash}"
                ));
            }

            log::warn!("Transaction attempt {attempt} timed out (hash: {tx_hash}), retrying...",);
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Spawns a worker task to process queued transactions in the background.
    /// The worker receives transaction requests, processes them, and sends back receipts.
    fn spawn_worker(mut rx: mpsc::Receiver<QueueMessage>, queue: Arc<Self>) {
        let runtime = Builder::new_current_thread().enable_all().build().unwrap();

        // We have 2 options to spawn signing worker task: using `spawn` or `spawn_local`.
        // Using `spawn_local` can panic when called outside of LocalSet. That means that
        // we force library consumer to use actix runtime or to manually create LocalSet.
        // On the other hand using `spawn` will prevent consumer from using `spawn_local` in
        // signing function.
        // Spawning thread here might be overkill, but it's the only way to avoid affecting users.
        std::thread::spawn(move || {
            let local = LocalSet::new();

            local.spawn_local(async move {
                while let Some(msg) = rx.recv().await {
                    let QueueMessage {
                        request,
                        response_tx,
                    } = msg;

                    let result = match tokio::time::timeout(
                        queue.tx_config.transaction_receipt_timeout,
                        queue.process_transaction(request),
                    )
                    .await
                    {
                        Ok(result) => result,
                        Err(e) => Err(anyhow!(
                            "Transaction processing timed out (timeout: {}): {e}",
                            humantime::format_duration(queue.tx_config.transaction_receipt_timeout)
                        )),
                    };
                    let _ = response_tx.send(result);
                }
            });

            runtime.block_on(local);
        });
    }

    /// Gets all three nonce values for this account.
    /// Returns a NonceInfo struct containing last_used_nonce, next_pending_nonce, and current_blockchain_nonce.
    async fn get_nonces(&self, address: Address) -> Result<NonceInfo> {
        // Get last used nonce from stored value
        let last_used_nonce = *self.last_used_nonce.lock().unwrap();

        // Get current blockchain nonce from get_nonce. This function includes only
        // transaction included in the latest block.
        let account_nonce = self.provider.get_nonce(address).await?;

        // Get next pending nonce from get_transaction_count. This function includes
        // pending transactions as well.
        let next_pending_nonce = self.provider.get_transaction_count(address).await?;

        Ok(NonceInfo {
            last_used_nonce,
            next_pending_nonce,
            account_nonce,
        })
    }

    /// Sets the last used nonce for this account.
    fn set_last_used_nonce(&self, nonce: u64) {
        let mut last_used = self.last_used_nonce.lock().unwrap();
        *last_used = Some(nonce);
    }

    /// Queues a transaction for processing and returns a channel to await the result.
    /// The transaction will be signed, sent, and the receipt returned asynchronously.
    async fn queue_transaction(&self, request: TransactionRequest) -> Result<TransactionChannel> {
        let (response_tx, response_rx) = oneshot::channel();
        let msg = QueueMessage {
            request,
            response_tx,
        };
        self.sender
            .send(msg)
            .await
            .map_err(|e| anyhow!("Failed to queue transaction: {}", e))?;
        Ok(TransactionChannel { response_rx })
    }

    /// Checks if there are pending transactions and waits for them to be mined until timeout.
    /// Returns an error if pending transactions won't be mined within the timeout.
    pub async fn wait_for_pending_transactions(
        &self,
        address: Address,
        timeout: Duration,
    ) -> Result<()> {
        let start_time = std::time::Instant::now();
        let mut last_pending_count = 0u64;

        loop {
            let nonce_info = self.get_nonces(address).await?;

            // Check if there are pending transactions using the same logic as pick_nonce
            let pending = nonce_info.pending_transactions_count();
            if pending == 0 {
                log::debug!("No pending transactions found for address {address}");
                return Ok(());
            }

            // Only log when the pending count changes
            if pending != last_pending_count {
                log::info!("Found {pending} pending transaction(s) (address {address}), waiting for them to be mined...");
                last_pending_count = pending;
            }

            // Check if we've exceeded the timeout
            if start_time.elapsed() >= timeout {
                bail!("Timeout: {pending} transaction(s) are still pending (address {address})");
            }

            // Wait a bit before checking again
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
}

/// An account with its signer.
/// Provides methods for sending transactions, funding, and interacting with Arkiv storage.
#[derive(Clone)]
pub struct Account {
    /// The account's signer for signing transactions.
    pub signer: Arc<Box<dyn TransactionSigner>>,
    /// The provider for making RPC calls.
    pub provider: ResilientProvider,
    /// The chain ID of the connected network.
    pub chain_id: u64,
    /// Transaction queue for managing transaction submissions.
    transaction_queue: Arc<TransactionQueue>,
    /// Transaction configuration for storage operations.
    tx_config: Arc<TransactionConfig>,
}

impl Account {
    /// Creates a new account with the given signer, provider, chain ID, and transaction config.
    /// Initializes a transaction queue for managing transaction submissions.
    pub fn new(
        signer: Box<dyn TransactionSigner>,
        provider: ResilientProvider,
        chain_id: u64,
        tx_config: Arc<TransactionConfig>,
    ) -> Self {
        let signer = Arc::new(signer);
        let transaction_queue =
            TransactionQueue::new(provider.clone(), signer.clone(), tx_config.clone());
        Self {
            signer,
            provider,
            chain_id,
            transaction_queue,
            tx_config,
        }
    }

    /// Returns the Ethereum address of this account.
    pub fn address(&self) -> Address {
        self.signer.address()
    }

    /// Validates the configured chain ID against the actual chain ID.
    /// If no chain ID is configured in TransactionConfig, this method does nothing.
    /// Returns the actual chain ID and an error if the configured chain ID doesn't match.
    pub async fn validate_chain_id(&self) -> Result<u64> {
        if let Some(configured_chain_id) = self.tx_config.chain_id {
            if configured_chain_id != self.chain_id {
                return Err(anyhow::anyhow!(
                    "Chain ID mismatch: configured {} but actual chain ID is {}",
                    configured_chain_id,
                    self.chain_id
                ));
            }
        }
        Ok(self.chain_id)
    }

    /// Sends a transaction with common fields filled in (from, chain_id).
    /// Queues the transaction for signing and submission, and awaits the receipt.
    pub async fn send_transaction(&self, mut tx: TransactionRequest) -> Result<TransactionReceipt> {
        // Validate chain ID if configured and get the actual chain ID
        let chain_id = self.validate_chain_id().await?;

        // Fill in common fields
        tx = tx.with_from(self.address()).with_chain_id(chain_id);

        // Queue the raw transaction (unsigned)
        let channel = self.transaction_queue.queue_transaction(tx).await?;
        channel.receipt().await
    }

    /// Creates and sends a storage transaction to the Arkiv contract.
    /// Encodes the transaction payload and submits it to the storage processor contract.
    pub async fn send_db_transaction(&self, tx: ArkivTransaction) -> Result<TransactionReceipt> {
        let mut data = Vec::new();
        tx.encode(&mut data);

        let tx = TransactionRequest::default()
            .with_to(ARKIV_STORAGE_PROCESSOR_ADDRESS)
            .with_gas_limit(self.tx_config.gas_limit)
            .with_max_priority_fee_per_gas(self.tx_config.max_priority_fee_per_gas)
            .with_max_fee_per_gas(self.tx_config.max_fee_per_gas)
            .with_input(data.to_vec());

        self.send_transaction(tx).await
    }

    /// Transfers ETH from this account to another address.
    /// Returns the transaction receipt after the transfer is mined.
    pub async fn transfer(&self, to: Address, value: BigDecimal) -> Result<TransactionReceipt> {
        let tx = TransactionRequest::default()
            .with_to(to)
            .with_value(eth_to_wei(value)?)
            .with_gas_limit(21_000)
            .with_max_priority_fee_per_gas(1_000_000)
            .with_max_fee_per_gas(20_000_000);

        self.send_transaction(tx).await
    }

    /// Funds an account by sending ETH from a node-managed account.
    /// This is typically used in development mode for test funding.
    pub async fn fund_account(&self, value: BigDecimal) -> anyhow::Result<TransactionReceipt> {
        let accounts = self.provider.get_accounts().await?;
        let funder = accounts[0];

        let nonce = self.provider.get_transaction_count(funder).await?;

        let tx = TransactionRequest::default()
            .with_to(self.address())
            .with_from(funder)
            .with_value(eth_to_wei(value)?)
            .with_nonce(nonce)
            .with_chain_id(self.chain_id)
            .with_gas_limit(21_000)
            .with_max_priority_fee_per_gas(1_000_000_000)
            .with_max_fee_per_gas(20_000_000_000);

        let pending = self
            .provider
            .send_transaction(tx)
            .await
            .map_err(|e| anyhow!("Failed to send transaction: {}", e))?;
        self.transaction_queue
            .get_receipt_with_retry(*pending.tx_hash())
            .await?
            .ok_or_else(|| anyhow!("Transaction receipt not found for funding transaction"))
    }

    /// Gets the account's ETH balance from the provider.
    /// Returns the balance as a U256 value.
    pub async fn get_balance(&self) -> anyhow::Result<U256> {
        Ok(self.provider.get_balance(self.address()).await?)
    }
}

/// Gets a transaction receipt with retries for "transaction indexing is in progress" errors.
/// Waits until the transaction is indexed and the receipt is available, or returns an error.
/// If a timeout is provided and no receipt is received within that time, returns None.
pub async fn get_receipt(
    provider: &ResilientProvider,
    tx_hash: Hash,
    timeout_duration: Option<Duration>,
    confirmations: u64,
) -> anyhow::Result<Option<TransactionReceipt>> {
    let start_time = std::time::Instant::now();
    let _ = provider.wait_for_indexing(tx_hash, timeout_duration).await;

    loop {
        // Recalculate timeout in case it decreased during retries
        let elapsed = start_time.elapsed();
        let remaining_timeout = timeout_duration.map(|timeout| timeout.saturating_sub(elapsed));

        if let Some(remaining_timeout) = remaining_timeout {
            if remaining_timeout == Duration::ZERO {
                return Ok(None);
            }
        }

        let config = PendingTransactionConfig::new(tx_hash)
            .with_required_confirmations(confirmations)
            .with_timeout(remaining_timeout);
        provider
            .watch_for_confirmation(config)
            .await
            .map_err(|e| anyhow!("Failed watching for {tx_hash} confirmation: {e}"))?;

        match provider
            .get_transaction_receipt(tx_hash)
            .await
            .map_err(|e| anyhow!("Failed to get transaction receipt: {e}"))?
        {
            Some(receipt) => {
                log::info!(
                    "Transaction {tx_hash} was included in a block {:?} ({:?}). Waiting for {confirmations} confirmations.",
                    receipt.block_number,
                    receipt.block_hash.map(|b| hex::encode(b))
                );
                return Ok(Some(receipt));
            }
            None => {
                log::trace!("Getting receipt returned None for transaction: {tx_hash}");
                tokio::time::sleep(Duration::from_millis(200)).await;
                continue;
            }
        }
    }
}
