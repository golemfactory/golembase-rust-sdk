use alloy::{
    primitives::{Address, B256},
    rpc::types::{BlockNumberOrTag, Filter, FilterBlockOption, FilterSet, Log},
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, RwLock};

use arkiv_sdk::account::ARKIV_STORAGE_PROCESSOR_ADDRESS;
use arkiv_sdk::events::{
    arkiv_storage_entity_created, arkiv_storage_entity_deleted,
    arkiv_storage_entity_updated,
};

use crate::{
    block::{Block, Transaction},
    display::EnableDisplay,
};
use crate::{display::display_topics, entity_db::Entity};

/// Event structure that stores all information needed to recreate an alloy Log
#[derive(Debug, Clone)]
pub struct LogEvent {
    /// The contract address that emitted the event
    pub address: Address,
    /// The event topic (hash of the event signature)
    pub topic: B256,
    /// The entity ID (first indexed parameter)
    pub entity_id: B256,
    /// Additional data (second indexed parameter for some events)
    pub additional_data: Option<B256>,
    /// The block number where the event occurred
    pub block_number: u64,
    /// The transaction hash that triggered the event
    pub transaction_hash: B256,
    /// The transaction index within the block
    pub transaction_index: u64,
    /// The log index within the transaction
    pub log_index: u64,
    /// The block hash
    pub block_hash: B256,
}

impl Into<Log> for LogEvent {
    fn into(self) -> Log {
        let mut topics = vec![self.topic];
        topics.push(self.entity_id);
        if let Some(data) = self.additional_data {
            topics.push(data);
        }

        let inner =
            alloy::primitives::Log::new(self.address, topics, alloy::primitives::Bytes::new())
                .unwrap_or_else(|| alloy::primitives::Log::empty());

        Log {
            inner,
            block_timestamp: None, // We don't have timestamp in our LogEvent
            block_number: Some(self.block_number),
            transaction_hash: Some(self.transaction_hash),
            transaction_index: Some(self.transaction_index),
            log_index: Some(self.log_index),
            block_hash: Some(self.block_hash),
            removed: false,
        }
    }
}

/// Simple filter for event subscriptions
#[derive(Debug, Clone, derive_more::Display)]
#[display("EventFilter(topics: {}, from_block: {}, to_block: {})", display_topics(topics), from_block.display(), to_block.display())]
pub struct EventFilter {
    /// Contract addresses to filter by
    pub addresses: FilterSet<Address>,
    /// Event topics to filter by
    pub topics: Vec<FilterSet<B256>>,
    /// Block range to filter by
    pub from_block: Option<u64>,
    pub to_block: Option<u64>,
}

/// Trait for handling entity events
#[async_trait::async_trait]
pub trait EntityEventHandler: Send + Sync {
    async fn on_entity_created(&self, entity: &Entity, block: &Block, transaction: &Transaction);
    async fn on_entity_updated(&self, entity: &Entity, block: &Block, transaction: &Transaction);
    async fn on_entity_removed(&self, entity: &Entity, block: &Block, transaction: &Transaction);
    /// Finish processing a block and emit all collected logs with proper indices
    async fn finish_block(&self, block_number: u64);
}

/// Information about a subscription including its filter
#[derive(Clone)]
struct SubscriptionInfo {
    sender: broadcast::Sender<LogEvent>,
    filter: Option<EventFilter>,
}

impl SubscriptionInfo {
    /// Send a log event to the subscription with logging
    fn emit_event(&self, log_event: &LogEvent) {
        let _ = self.sender.send(log_event.clone());
        log::info!(
            "Sent event to subscription: block={}, tx={}, log_index={}, entity_id={}",
            log_event.block_number,
            log_event.transaction_hash,
            log_event.log_index,
            log_event.entity_id
        );
    }
}

/// Inner state of the EventEmitter, wrapped in a single RwLock
#[derive(Default)]
struct EventEmitterState {
    subscriptions: HashMap<String, SubscriptionInfo>,
    /// Pending logs per block, waiting to be emitted with proper indices
    pending_logs: HashMap<u64, Vec<LogEvent>>,
}

/// Event emission system for GolemBase mock
#[derive(Clone, Default)]
pub struct EventEmitter {
    state: Arc<RwLock<EventEmitterState>>,
}

impl EventEmitter {
    /// Create a new event emitter
    pub fn new() -> Self {
        Self::default()
    }

    /// Generate a unique subscription ID
    fn generate_subscription_id() -> String {
        format!(
            "events_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )
    }

    /// Get the latest block number from the state
    async fn get_latest_block_number(&self) -> u64 {
        let state = self.state.read().await;
        state.pending_logs.keys().max().copied().unwrap_or(0)
    }

    /// Extract block range from Filter
    async fn extract_block_range(
        &self,
        filter: &Filter,
    ) -> anyhow::Result<(Option<u64>, Option<u64>)> {
        let latest = self.get_latest_block_number().await;

        match &filter.block_option {
            FilterBlockOption::Range {
                from_block,
                to_block,
            } => {
                let from = from_block.and_then(|b| self.extract_block_number(&b, latest));
                let to = to_block.and_then(|b| self.extract_block_number(&b, latest));
                Ok((from, to))
            }
            FilterBlockOption::AtBlockHash(_) => {
                // Single block hash - we can't resolve the hash to a block number
                anyhow::bail!("[Not implemented] Cannot return events for a block hash")
            }
        }
    }

    /// Extract block number from BlockNumberOrTag
    fn extract_block_number(
        &self,
        block_tag: &BlockNumberOrTag,
        latest_block_number: u64,
    ) -> Option<u64> {
        match block_tag {
            BlockNumberOrTag::Number(n) => Some(*n),
            BlockNumberOrTag::Earliest => Some(0),
            // Latest, Finalized, Safe all map to the current latest block number
            // since we treat them as equivalent in our mock implementation
            BlockNumberOrTag::Latest | BlockNumberOrTag::Finalized | BlockNumberOrTag::Safe => {
                Some(latest_block_number)
            }
            // Pending is relative to current state - we can't extract specific numbers
            BlockNumberOrTag::Pending => None,
        }
    }

    /// Add a log event to the pending logs for a block
    async fn add_pending_log(&self, mut log_event: LogEvent) {
        let mut state = self.state.write().await;
        let block_logs = state
            .pending_logs
            .entry(log_event.block_number)
            .or_insert_with(Vec::new);

        // Set log index to the current position in the vector
        log_event.log_index = block_logs.len() as u64;
        block_logs.push(log_event);
    }

    /// Emit a log event to all subscribers, applying their individual filters
    async fn emit_log_event(&self, log_event: LogEvent) {
        let state = self.state.read().await;
        for (_, subscription_info) in state.subscriptions.iter() {
            // Check if event matches the subscription's filter
            let should_send = match &subscription_info.filter {
                Some(filter) => self.log_event_matches_filter(&log_event, filter),
                None => true,
            };

            if should_send {
                subscription_info.emit_event(&log_event);
            }
        }
    }

    /// Subscribe to events with an optional filter
    pub async fn subscribe_to_events(
        &self,
        filter: Option<Filter>,
    ) -> anyhow::Result<broadcast::Receiver<LogEvent>> {
        let subscription_id = Self::generate_subscription_id();
        let (sender, receiver) = broadcast::channel(1000); // Buffer up to 1000 events

        let event_filter = match filter {
            Some(f) => {
                let (from_block, to_block) = self.extract_block_range(&f).await?;
                Some(EventFilter {
                    addresses: f.address.clone(),
                    topics: f.topics.iter().cloned().collect(),
                    from_block,
                    to_block,
                })
            }
            None => None,
        };

        let subscription_info = SubscriptionInfo {
            sender: sender.clone(),
            filter: event_filter.clone(),
        };

        let mut state = self.state.write().await;
        state
            .subscriptions
            .insert(subscription_id, subscription_info.clone());

        log::info!(
            "Created subscription for events with filter: {}",
            event_filter.display()
        );

        // Emit events from past blocks if they match the filter
        if let Some(filter) = &event_filter {
            log::info!("Emitting events from past blocks using filter: {filter}");
            self.emit_past_blocks(&subscription_info, filter, &state)
                .await;
        }

        Ok(receiver)
    }

    /// Emit events from past blocks that match the filter
    async fn emit_past_blocks(
        &self,
        subscription_info: &SubscriptionInfo,
        filter: &EventFilter,
        state: &EventEmitterState,
    ) {
        for (_, logs) in &state.pending_logs {
            // Emit logs that match the filter (which includes block range)
            for log_event in logs {
                if self.log_event_matches_filter(log_event, filter) {
                    subscription_info.emit_event(log_event);
                }
            }
        }
    }

    /// Check if a log event matches the given filter
    fn log_event_matches_filter(&self, log_event: &LogEvent, filter: &EventFilter) -> bool {
        // Check block range first
        if let Some(from_block) = filter.from_block {
            if log_event.block_number < from_block {
                return false;
            }
        }
        if let Some(to_block) = filter.to_block {
            if log_event.block_number > to_block {
                return false;
            }
        }

        // Check if filter has any address restrictions
        if !filter.addresses.is_empty() {
            // Check if the log event address matches any of the filter addresses
            let address_matches = filter.addresses.contains(&log_event.address);
            if !address_matches {
                return false;
            }
        }

        // Check if filter has any topic restrictions
        if !filter.topics.is_empty() {
            // Check if the event topic matches any of the filter topics
            // Topics is a Vec<FilterSet<B256>>, so we need to check if any FilterSet contains the topic
            let topic_matches = filter
                .topics
                .iter()
                .any(|topic_set| topic_set.contains(&log_event.topic));
            if !topic_matches {
                return false;
            }
        }

        true
    }
}

#[async_trait::async_trait]
impl EntityEventHandler for EventEmitter {
    async fn on_entity_created(&self, entity: &Entity, block: &Block, transaction: &Transaction) {
        let transaction_index = block.find_transaction_index(&transaction.hash).unwrap_or(0);

        let log_event = LogEvent {
            address: ARKIV_STORAGE_PROCESSOR_ADDRESS,
            topic: arkiv_storage_entity_created(),
            entity_id: entity.key,
            additional_data: None,
            block_number: block.header.block_number,
            transaction_hash: transaction.hash,
            transaction_index,
            log_index: 0, // Will be set correctly in add_pending_log
            block_hash: block.header.block_hash,
        };

        self.add_pending_log(log_event).await;
    }

    async fn on_entity_updated(&self, entity: &Entity, block: &Block, transaction: &Transaction) {
        let transaction_index = block.find_transaction_index(&transaction.hash).unwrap_or(0);

        let log_event = LogEvent {
            address: ARKIV_STORAGE_PROCESSOR_ADDRESS,
            topic: arkiv_storage_entity_updated(),
            entity_id: entity.key,
            additional_data: None,
            block_number: block.header.block_number,
            transaction_hash: transaction.hash,
            transaction_index,
            log_index: 0, // Will be set correctly in add_pending_log
            block_hash: block.header.block_hash,
        };

        self.add_pending_log(log_event).await;
    }

    async fn on_entity_removed(&self, entity: &Entity, block: &Block, transaction: &Transaction) {
        let transaction_index = block.find_transaction_index(&transaction.hash).unwrap_or(0);

        let log_event = LogEvent {
            address: ARKIV_STORAGE_PROCESSOR_ADDRESS,
            topic: arkiv_storage_entity_deleted(),
            entity_id: entity.key,
            additional_data: None,
            block_number: block.header.block_number,
            transaction_hash: transaction.hash,
            transaction_index,
            log_index: 0, // Will be set correctly in add_pending_log
            block_hash: block.header.block_hash,
        };

        self.add_pending_log(log_event).await;
    }

    async fn finish_block(&self, block_number: u64) {
        let logs = {
            let state = self.state.read().await;
            state.pending_logs.get(&block_number).cloned()
        };

        if let Some(logs) = logs {
            // Emit all logs in the order they were added (already correct order)
            for log_event in logs {
                self.emit_log_event(log_event).await;
            }
        }
    }
}
