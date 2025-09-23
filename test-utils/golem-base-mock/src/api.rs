use alloy::primitives::{Address, Bytes, B256, U256};
use alloy::rpc::types::{
    Block, BlockId, BlockNumberOrTag, Filter, Transaction, TransactionReceipt, TransactionRequest,
};
use golem_base_sdk::entity::Entity;
use golem_base_sdk::rpc::{EntityMetaData, SearchResult};
use jsonrpsee::core::{RpcResult, SubscriptionResult};
use jsonrpsee::proc_macros::rpc;

/// Mock implementation of Ethereum RPC methods
#[rpc(server, namespace = "eth")]
pub trait EthRpc {
    #[method(name = "getTransactionCount")]
    async fn get_transaction_count(
        &self,
        address: Address,
        block: Option<BlockId>,
    ) -> RpcResult<U256>;

    #[method(name = "getTransactionReceipt")]
    async fn get_transaction_receipt(&self, hash: B256) -> RpcResult<Option<TransactionReceipt>>;

    #[method(name = "getProof")]
    async fn get_proof(
        &self,
        address: Address,
        keys: Vec<B256>,
        block: Option<BlockId>,
    ) -> RpcResult<serde_json::Value>;

    #[method(name = "getBalance")]
    async fn get_balance(&self, address: Address, block: Option<BlockId>) -> RpcResult<U256>;

    #[method(name = "accounts")]
    async fn accounts(&self) -> RpcResult<Vec<Address>>;

    #[method(name = "getAccounts")]
    async fn get_accounts(&self) -> RpcResult<Vec<Address>>;

    #[method(name = "sendTransaction")]
    async fn send_transaction(&self, transaction: TransactionRequest) -> RpcResult<B256>;

    #[method(name = "sendRawTransaction")]
    async fn send_raw_transaction(&self, data: Bytes) -> RpcResult<B256>;

    #[method(name = "chainId")]
    async fn chain_id(&self) -> RpcResult<U256>;

    #[method(name = "getTransactionByHash")]
    async fn get_transaction_by_hash(&self, hash: B256) -> RpcResult<Option<Transaction>>;

    #[method(name = "syncing")]
    async fn syncing(&self) -> RpcResult<bool>;

    #[method(name = "getBlockByNumber")]
    async fn get_block_by_number(
        &self,
        block: BlockNumberOrTag,
        full: Option<bool>,
    ) -> RpcResult<Option<Block>>;

    #[method(name = "estimateGas")]
    async fn estimate_gas(&self, call_request: serde_json::Value) -> RpcResult<U256>;

    #[method(name = "feeHistory")]
    async fn fee_history(
        &self,
        block_count: U256,
        newest_block: BlockId,
        reward_percentiles: Option<Vec<f64>>,
    ) -> RpcResult<serde_json::Value>;

    #[method(name = "gasPrice")]
    async fn gas_price(&self) -> RpcResult<U256>;

    #[method(name = "blockNumber")]
    async fn block_number(&self) -> RpcResult<U256>;

    #[subscription(name = "subscribe", item = Event)]
    async fn subscribe(
        &self,
        subscription_type: String,
        filter: Option<Filter>,
    ) -> SubscriptionResult;
}

/// Mock implementation of GolemBase RPC methods
#[rpc(server, namespace = "golembase")]
pub trait GolemBaseRpc {
    #[method(name = "getEntity")]
    async fn get_entity(&self, key: B256) -> RpcResult<Option<Entity>>;

    #[method(name = "getEntityMetaData")]
    async fn get_entity_metadata(&self, key: B256) -> RpcResult<EntityMetaData>;

    #[method(name = "getEntityCount")]
    async fn get_entity_count(&self) -> RpcResult<u64>;

    #[method(name = "getAllEntityKeys")]
    async fn get_all_entity_keys(&self) -> RpcResult<Option<Vec<B256>>>;

    #[method(name = "getEntitiesOfOwner")]
    async fn get_entities_of_owner(&self, address: Address) -> RpcResult<Option<Vec<B256>>>;

    #[method(name = "getStorageValue")]
    async fn get_storage_value(&self, keys: B256) -> RpcResult<String>;

    #[method(name = "queryEntities")]
    async fn query_entities(&self, query: String) -> RpcResult<Vec<SearchResult>>;

    #[method(name = "getEntitiesToExpireAtBlock")]
    async fn get_entities_to_expire_at_block(
        &self,
        block_number: u64,
    ) -> RpcResult<Option<Vec<B256>>>;
}
