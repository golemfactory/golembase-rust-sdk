use alloy::primitives::{Address, Bytes, B256};
use std::sync::Arc;

use crate::{
    block::{Block, Transaction, TransactionLog},
    events::{EntityEventHandler, LogEvent},
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
        let expiration_block = entity
            .expires_at
            .unwrap_or(self.block.header.block_number + entity.btl);
        let payload = LogEvent::encode_creation_payload(expiration_block);
        // Emit event
        self.event_handler
            .on_entity_created(entity, &self.block, transaction)
            .await;

        // Create log and add to block
        let create_log = TransactionLog::new_entity_log(
            transaction,
            arkiv_sdk::events::arkiv_storage_entity_created(),
            entity.key,
            entity.owner,
            payload,
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
        old_expiration: u64,
        new_expiration: u64,
    ) {
        let payload = LogEvent::encode_update_payload(old_expiration, new_expiration);
        // Emit event
        self.event_handler
            .on_entity_updated(
                entity,
                &self.block,
                transaction,
                old_expiration,
                new_expiration,
            )
            .await;

        // Create log and add to block
        let update_log = TransactionLog::new_entity_log(
            transaction,
            arkiv_sdk::events::arkiv_storage_entity_updated(),
            entity.key,
            entity.owner,
            payload,
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
        let delete_log = TransactionLog::new_entity_log(
            transaction,
            arkiv_sdk::events::arkiv_storage_entity_deleted(),
            entity.key,
            entity.owner,
            Bytes::new(),
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
        owner: Address,
        old_expiration: u64,
        new_expiration: u64,
    ) {
        let payload = LogEvent::encode_extend_payload(old_expiration, new_expiration);
        // Create log and add to block
        let extend_log = TransactionLog::new_entity_log(
            transaction,
            arkiv_sdk::events::arkiv_storage_entity_ttl_extended(),
            entity_key,
            owner,
            payload,
        );
        self.block.transaction_logs.push(extend_log);

        log::info!(
            "Entity extended: 0x{:x}, new expiration: {}, tx: 0x{:x}",
            entity_key,
            new_expiration,
            transaction.hash
        );
    }

    pub fn build(self) -> Arc<Block> {
        Arc::new(self.block)
    }
}
