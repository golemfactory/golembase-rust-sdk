use alloy::eips::BlockNumberOrTag;
use alloy::primitives::{Address, B256};
use anyhow::Result;
use bigdecimal::BigDecimal;
use std::env;

use arkiv_sdk::client::ArkivClient;
use arkiv_sdk::entity::Hash;

pub mod entity_set;
pub mod arkiv;

/// Default URL for Arkiv node in tests
pub const ARKIV_URL: &str = "http://localhost:8545";

/// Default TTL value for test entities
pub const TEST_TTL: u64 = 30;

/// Initializes the logger for tests
pub fn init_logger(should_init: bool) {
    // Check if TEST_ENABLE_ALL_LOGS_OVERRIDE environment variable overrides the should_init parameter
    let env_override = env::var("TEST_ENABLE_ALL_LOGS_OVERRIDE")
        .map(|val| val.parse::<bool>().unwrap_or(false))
        .unwrap_or(false);
    let should_enable = should_init || env_override;

    if should_enable {
        if let Ok(_env) = env::var("RUST_LOG") {
            env_logger::try_init_from_env(env_logger::Env::default()).ok();
        } else {
            env_logger::builder()
                .filter_level(log::LevelFilter::Debug)
                .format_timestamp(Some(env_logger::TimestampPrecision::Millis))
                .try_init()
                .ok();
        }
    }
}

/// Removes all existing entities from the Arkiv node
pub async fn cleanup_entities(client: &ArkivClient, account: Address) -> Result<()> {
    let all_entity_keys = client.get_all_entity_keys().await?;
    log::info!("Removing all existing entities: {:?}", all_entity_keys);
    if !all_entity_keys.is_empty() {
        client.remove_entries(account, all_entity_keys).await?;
    }
    Ok(())
}

/// Creates a new test account with initial funding
pub async fn create_test_account(client: &ArkivClient) -> Result<Address> {
    let account = client.account_generate("test123").await?;
    let fund_tx = client.fund(account, BigDecimal::from(1)).await?;
    let balance = client.get_balance(account).await?;

    log::info!("Account {account} funded with transaction: {fund_tx} and balance {balance}");
    Ok(account)
}

/// Finds the transaction that created an entry by searching backwards through blocks.
/// Returns (transaction_hash, block_number) or None if not found.
pub async fn find_entry_creation_transaction(
    client: &ArkivClient,
    entry_id: Hash,
) -> Result<Option<(B256, u64)>> {
    let current_block = client.get_current_block_number().await?;
    for block_number in (1..=current_block).rev() {
        let block = client
            .get_rpc_client()
            .get_block_by_number(BlockNumberOrTag::Number(block_number))
            .await?
            .ok_or_else(|| anyhow::anyhow!("Block {} not found", block_number))?;

        for tx in block.transactions.hashes() {
            if let Ok(Some(receipt)) = client.get_rpc_client().get_transaction_receipt(tx).await {
                if let Ok(log_entry_id) = ArkivClient::extract_entity_id(receipt.logs()) {
                    if log_entry_id == entry_id {
                        return Ok(Some((tx, block_number)));
                    }
                }
            }
        }
    }
    Ok(None)
}
