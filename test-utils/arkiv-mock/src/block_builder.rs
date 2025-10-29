use alloy::primitives::B256;
use std::sync::Arc;

use crate::{
    block::{Block, Transaction, TransactionLog},
    events::EntityEventHandler,
};

/// Builder for modifying a block by adding logs and emitting events in a unified way.
/// Ensures that log collection and event emission stay in sync.
pub struct BlockBuilder {
    event_handler: Arc<dyn EntityEventHandler>,
    pub block: Block,
}

impl BlockBuilder {
    /// Create a new BlockBuilder
    pub fn new(event_handler: Arc<dyn EntityEventHandler>, block: Block) -> Self {
        Self {
            event_handler,
            block,
        }
    }

    /// Add a log and emit the corresponding event for entity creation
    pub async fn log_entity_created(
        &mut self,
        transaction: &Arc<Transaction>,
        entity: &crate::entity_db::Entity,
    ) {
        // Emit event
        self.event_handler
            .on_entity_created(entity, &self.block, transaction)
            .await;

        // Create log and add to block
        let create_log = TransactionLog::create_entity_log(
            transaction,
            arkiv_sdk::events::arkiv_storage_entity_created(),
            entity.key,
        );
        self.block.transaction_logs.push(create_log);

        log::info!(
            "Entity created: 0x{:x}, owner: 0x{:x}, tx: 0x{:x}, expires_at: {:?}",
            entity.key,
            entity.owner,
            transaction.hash,
            entity.expires_at
        );
    }

    /// Add a log and emit the corresponding event for entity update
    pub async fn log_entity_updated(
        &mut self,
        transaction: &Arc<Transaction>,
        entity: &crate::entity_db::Entity,
    ) {
        // Emit event
        self.event_handler
            .on_entity_updated(entity, &self.block, transaction)
            .await;

        // Create log and add to block
        let update_log = TransactionLog::create_entity_log(
            transaction,
            arkiv_sdk::events::arkiv_storage_entity_updated(),
            entity.key,
        );
        self.block.transaction_logs.push(update_log);

        log::info!(
            "Entity updated: 0x{:x}, tx: 0x{:x}",
            entity.key,
            transaction.hash
        );
    }

    /// Add a log and emit the corresponding event for entity removal
    pub async fn log_entity_removed(
        &mut self,
        transaction: &Arc<Transaction>,
        entity: &crate::entity_db::Entity,
    ) {
        // Emit event
        self.event_handler
            .on_entity_removed(entity, &self.block, transaction)
            .await;

        // Create log and add to block
        let delete_log = TransactionLog::create_entity_log(
            transaction,
            arkiv_sdk::events::arkiv_storage_entity_deleted(),
            entity.key,
        );
        self.block.transaction_logs.push(delete_log);

        log::info!(
            "Entity deleted: 0x{:x}, owner: 0x{:x}, tx: 0x{:x}",
            entity.key,
            entity.owner,
            transaction.hash
        );
    }

    /// Add a log and emit event for entity expiration (special case for expired entities)
    pub async fn log_entity_expired(
        &mut self,
        transaction: &Arc<Transaction>,
        entity: &crate::entity_db::Entity,
    ) {
        log::info!("Entity expired: 0x{:x}", entity.key);
        self.log_entity_removed(transaction, entity).await;
    }

    /// Add a log for entity TTL extension (no event needed as it's not a state change)
    pub fn log_entity_ttl_extended(
        &mut self,
        transaction: &Arc<Transaction>,
        entity_key: B256,
        number_of_blocks: u64,
    ) {
        // Create log and add to block
        let extend_log = TransactionLog::create_entity_log(
            transaction,
            arkiv_sdk::events::arkiv_storage_entity_ttl_extended(),
            entity_key,
        );
        self.block.transaction_logs.push(extend_log);

        log::info!(
            "Entity extended: 0x{:x}, new BTL: {}, tx: 0x{:x}",
            entity_key,
            number_of_blocks,
            transaction.hash
        );
    }

    pub fn build(self) -> Arc<Block> {
        Arc::new(self.block)
    }
}
