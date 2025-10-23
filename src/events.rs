use alloy::eips::BlockNumberOrTag;
use alloy::primitives::{keccak256, B256};
use alloy::providers::{DynProvider, Provider, ProviderBuilder, WsConnect};
use alloy::rpc::types::eth::Filter;
use alloy::rpc::types::Log;
use alloy::transports::http::reqwest::Url;
use anyhow::Result;
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;
use std::pin::Pin;

use crate::account::ARKIV_STORAGE_PROCESSOR_ADDRESS;
use crate::entity::Hash;

/// Returns the event signature hash for entity creation logs.
/// Used to identify `GolemBaseStorageEntityCreated` events in the blockchain logs.
pub fn arkiv_storage_entity_created() -> B256 {
    keccak256(b"GolemBaseStorageEntityCreated(uint256,uint256)")
}

/// Returns the event signature hash for entity deletion logs.
/// Used to identify `GolemBaseStorageEntityDeleted` events in the blockchain logs.
pub fn arkiv_storage_entity_deleted() -> B256 {
    keccak256(b"GolemBaseStorageEntityDeleted(uint256)")
}

/// Returns the event signature hash for entity update logs.
/// Used to identify `GolemBaseStorageEntityUpdated` events in the blockchain logs.
pub fn arkiv_storage_entity_updated() -> B256 {
    keccak256(b"GolemBaseStorageEntityUpdated(uint256,uint256)")
}

/// Returns the event signature hash for TTL extension logs.
/// Used to identify `GolemBaseStorageEntityBTLExtended` events in the blockchain logs.
pub fn arkiv_storage_entity_ttl_extended() -> B256 {
    keccak256(b"GolemBaseStorageEntityBTLExtended(uint256,uint256)")
}

/// Represents an Arkiv event parsed from the blockchain log.
/// Used to distinguish between entity creation, update, and removal events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    /// Entity was created.
    /// Contains the entity ID, block number, and transaction hash.
    EntityCreated {
        /// The ID of the created entity
        entity_id: Hash,
        /// The block number where the event occurred
        block_number: u64,
        /// The transaction hash that triggered the event
        transaction_hash: Hash,
    },
    /// Entity was updated.
    /// Contains the entity ID, block number, and transaction hash.
    EntityUpdated {
        /// The ID of the updated entity
        entity_id: Hash,
        /// The block number where the event occurred
        block_number: u64,
        /// The transaction hash that triggered the event
        transaction_hash: Hash,
    },
    /// Entity was removed.
    /// Contains the entity ID, block number, and transaction hash.
    EntityRemoved {
        /// The ID of the removed entity
        entity_id: Hash,
        /// The block number where the event occurred
        block_number: u64,
        /// The transaction hash that triggered the event
        transaction_hash: Hash,
    },
}

impl TryFrom<Log> for Event {
    type Error = anyhow::Error;

    /// Attempts to parse a blockchain log into a `Event`.
    /// Returns an error if required fields are missing or the event type is unknown.
    fn try_from(log: Log) -> Result<Self> {
        let block_number = log
            .block_number
            .ok_or_else(|| anyhow::anyhow!("Missing block number"))?;
        let transaction_hash = log
            .transaction_hash
            .ok_or_else(|| anyhow::anyhow!("Missing transaction hash"))?;

        if log.topics().len() < 2 {
            return Err(anyhow::anyhow!("Missing entity ID in event"));
        }

        let entity_id = Hash::from(log.topics()[1]);
        let transaction_hash = Hash::from(transaction_hash);

        match log.topics()[0] {
            topic if topic == arkiv_storage_entity_created() => Ok(Event::EntityCreated {
                entity_id,
                block_number,
                transaction_hash,
            }),
            topic if topic == arkiv_storage_entity_updated() => Ok(Event::EntityUpdated {
                entity_id,
                block_number,
                transaction_hash,
            }),
            topic if topic == arkiv_storage_entity_deleted() => Ok(Event::EntityRemoved {
                entity_id,
                block_number,
                transaction_hash,
            }),
            _ => Err(anyhow::anyhow!("Unknown event topic")),
        }
    }
}

/// Client for subscribing to and streaming Arkiv events from the blockchain.
/// Provides methods to connect to a node and receive event streams for entity changes.
pub struct EventsClient {
    provider: DynProvider,
}

impl EventsClient {
    /// Creates a new `EventsClient` by connecting to the given websocket `Url`.
    /// Establishes a connection to the blockchain node for event streaming.
    pub async fn new(url: Url) -> anyhow::Result<Self> {
        log::debug!("Connecting to websocket provider: {}", url);

        let provider = ProviderBuilder::new()
            .connect_ws(WsConnect::new(url.clone()))
            .await?
            .erased();

        log::info!("Connected to websocket provider: {}", url);
        Ok(Self { provider })
    }

    /// Listens for Arkiv events from the blockchain, starting from the latest block.
    /// Returns a stream of parsed `Event` items that can be processed asynchronously.
    pub async fn events_stream<'a>(
        &'a self,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<Event>> + Send + 'a>>> {
        let filter = self.create_event_filter(BlockNumberOrTag::Latest);
        self.create_stream_from_filter(filter).await
    }

    /// Listens for Arkiv events starting from a specific block number.
    /// Returns a stream of parsed `Event` items from the given block onward.
    ///
    /// # Arguments
    /// * `block` - The block number to start listening for events from.
    pub async fn events_stream_from_block<'a>(
        &'a self,
        block: u64,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<Event>> + Send + 'a>>> {
        let filter = self.create_event_filter(BlockNumberOrTag::Number(block));
        self.create_stream_from_filter(filter).await
    }

    /// Creates a filter for Arkiv events, specifying the contract address and event signatures.
    fn create_event_filter(&self, block: BlockNumberOrTag) -> Filter {
        Filter::new()
            .address(ARKIV_STORAGE_PROCESSOR_ADDRESS)
            .from_block(block)
            .event_signature(vec![
                arkiv_storage_entity_created(),
                arkiv_storage_entity_updated(),
                arkiv_storage_entity_deleted(),
            ])
    }

    /// Creates a stream of events from a filter.
    /// Subscribes to logs matching the filter and maps them to `Event` values.
    async fn create_stream_from_filter<'a>(
        &'a self,
        filter: Filter,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<Event>> + Send + 'a>>> {
        let subscription = self.provider.subscribe_logs(&filter).await?;
        Ok(Box::pin(subscription.into_stream().map(Event::try_from)))
    }
}
