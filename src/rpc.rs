use alloy::primitives::Address;
use alloy::rpc::json_rpc::{RpcRecv, RpcSend};
use alloy_json_rpc::RpcError as AlloyError;
use anyhow::anyhow;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use bytes::Bytes;
use displaydoc::Display;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt::Debug;
use thiserror::Error;

use crate::resilient_provider::RpcError;
use crate::{GolemBaseClient, Hash, NumericAnnotation, StringAnnotation};

/// Available columns for query results.
pub const COLUMNS: &[&str] = &[
    KEY_COLUMN,
    PAYLOAD_COLUMN,
    EXPIRES_AT_COLUMN,
    OWNER_ADDRESS_COLUMN,
];

/// Column identifiers for query results.
pub const KEY_COLUMN: &str = "key";
pub const PAYLOAD_COLUMN: &str = "payload";
pub const EXPIRES_AT_COLUMN: &str = "expires_at";
pub const OWNER_ADDRESS_COLUMN: &str = "owner_address";

/// Represents errors that can occur in the GolemBase RPC module.
/// Used to wrap and describe errors from RPC requests, decoding, or deserialization.
#[derive(Debug, Display, Error)]
pub enum Error {
    /// Failed to send the RPC request: {0}
    RpcRequestError(String),
    /// Failed to decode the base64-encoded storage value: {0}
    Base64DecodeError(String),
    /// Failed to deserialize the RPC response: {0}
    ResponseDeserializationError(String),
    /// Unexpected error occurred: {0}
    UnexpectedError(String),
}

/// Type representing metadata for an entity.
/// Contains information such as expiration, payload, annotations, and owner.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityMetaData {
    /// The block number at which the entity expires.
    pub expires_at_block: Option<u64>,
    /// The payload associated with the entity.
    pub payload: Option<String>,
    /// String annotations for the entity.
    pub string_annotations: Vec<StringAnnotation>,
    /// Numeric annotations for the entity.
    pub numeric_annotations: Vec<NumericAnnotation>,
    /// The owner of the entity.
    pub owner: Address,
}

/// Represents a single search result from a query.
/// Contains the entity key, value (decoded from base64), expiration, owner, and annotations.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchResult {
    #[serde(rename = "key")]
    pub key: Hash,
    #[serde(
        rename = "value",
        deserialize_with = "deserialize_optional_base64",
        serialize_with = "serialize_optional_base64"
    )]
    pub value: Option<Bytes>,
    #[serde(rename = "expires_at")]
    pub expires_at: u64,
    #[serde(rename = "owner")]
    pub owner: Address,
    #[serde(rename = "string_annotations")]
    pub string_annotations: Vec<StringAnnotation>,
    #[serde(rename = "numeric_annotations")]
    pub numeric_annotations: Vec<NumericAnnotation>,
}

/// Options for querying entities in GolemBase.
/// Controls which columns to return, whether to include annotations, and at which block to query.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueryOptions {
    /// The block number at which to query entities.
    #[serde(rename = "at_block")]
    pub at_block: Option<u64>,
    /// Whether to include annotations in the query results.
    #[serde(rename = "include_annotations")]
    pub include_annotations: bool,
    /// The columns to include in the query results.
    #[serde(rename = "columns")]
    pub columns: Vec<String>,
}

impl Default for QueryOptions {
    fn default() -> Self {
        Self::empty()
    }
}

impl QueryOptions {
    /// Creates a new `QueryOptions` with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an empty `QueryOptions` with no columns selected.
    fn empty() -> Self {
        Self {
            at_block: None,
            include_annotations: false,
            columns: Vec::new(),
        }
    }

    /// Creates a `QueryOptions` with all columns selected and annotations enabled.
    pub fn with_all() -> Self {
        Self {
            at_block: None,
            include_annotations: true,
            columns: COLUMNS.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Sets the block number at which to query entities.
    pub fn at_block(mut self, at_block: u64) -> Self {
        self.at_block = Some(at_block);
        self
    }

    /// Sets whether to include annotations in query results.
    pub fn with_annotations(mut self, include_annotations: bool) -> Self {
        self.include_annotations = include_annotations;
        self
    }

    /// Sets the columns to include in query results.
    pub fn with_columns(mut self, columns: Vec<String>) -> Self {
        self.columns = columns;
        self
    }

    /// Includes the key column in query results.
    pub fn with_key(mut self) -> Self {
        if !self.columns.contains(&KEY_COLUMN.to_string()) {
            self.columns.push(KEY_COLUMN.to_string());
        }
        self
    }

    /// Includes the payload column in query results.
    pub fn with_payload(mut self) -> Self {
        if !self.columns.contains(&PAYLOAD_COLUMN.to_string()) {
            self.columns.push(PAYLOAD_COLUMN.to_string());
        }
        self
    }

    /// Includes the expires_at column in query results.
    pub fn with_expires_at(mut self) -> Self {
        if !self.columns.contains(&EXPIRES_AT_COLUMN.to_string()) {
            self.columns.push(EXPIRES_AT_COLUMN.to_string());
        }
        self
    }

    /// Includes the owner_address column in query results.
    pub fn with_owner_address(mut self) -> Self {
        if !self.columns.contains(&OWNER_ADDRESS_COLUMN.to_string()) {
            self.columns.push(OWNER_ADDRESS_COLUMN.to_string());
        }
        self
    }

    /// Excludes the key column from query results.
    pub fn exclude_key(mut self) -> Self {
        self.columns.retain(|col| col != KEY_COLUMN);
        self
    }

    /// Excludes the payload column from query results.
    pub fn exclude_payload(mut self) -> Self {
        self.columns.retain(|col| col != PAYLOAD_COLUMN);
        self
    }

    /// Excludes the expires_at column from query results.
    pub fn exclude_expires_at(mut self) -> Self {
        self.columns.retain(|col| col != EXPIRES_AT_COLUMN);
        self
    }

    /// Excludes the owner_address column from query results.
    pub fn exclude_owner_address(mut self) -> Self {
        self.columns.retain(|col| col != OWNER_ADDRESS_COLUMN);
        self
    }
}

/// Helper function to decode a base64 string into Bytes
fn decode_base64_string<'de, D>(s: &str) -> Result<Bytes, D::Error>
where
    D: serde::Deserializer<'de>,
{
    BASE64
        .decode(s)
        .map(Bytes::from)
        .map_err(serde::de::Error::custom)
}

/// Helper for deserializing base64-encoded storage values.
/// Used to decode entity values returned from the RPC API.
pub fn deserialize_base64<'de, D>(deserializer: D) -> Result<Bytes, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    decode_base64_string::<D>(&s)
}

/// Serialize Bytes as base64 string
pub fn serialize_base64<S>(value: &Bytes, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&BASE64.encode(value))
}

/// Helper for deserializing optional base64-encoded storage values.
/// Used to decode optional entity values returned from the RPC API.
pub fn deserialize_optional_base64<'de, D>(deserializer: D) -> Result<Option<Bytes>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)?
        .map(|str| decode_base64_string::<D>(&str))
        .transpose()
}

/// Serialize optional Bytes as base64 string
pub fn serialize_optional_base64<S>(value: &Option<Bytes>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match value {
        Some(bytes) => serialize_base64(bytes, serializer),
        None => serializer.serialize_none(),
    }
}

impl SearchResult {
    /// Converts the value to a UTF-8 string.
    /// Returns an error if the value is not valid UTF-8.
    pub fn value_as_string(&self) -> anyhow::Result<String> {
        String::from_utf8(
            self.value
                .clone()
                .ok_or(anyhow::anyhow!("Value is not present"))?
                .to_vec(),
        )
        .map_err(|e| anyhow::anyhow!("Failed to decode search result to string: {}", e))
    }
}

impl GolemBaseClient {
    /// Makes a JSON-RPC call to the GolemBase endpoint.
    /// Handles serialization, deserialization, and error mapping for RPC requests.
    pub(crate) async fn rpc_call<S: RpcSend, R: RpcRecv>(
        &self,
        method: impl Into<Cow<'static, str>>,
        params: S,
    ) -> Result<R, Error> {
        let method = method.into();
        log::debug!("RPC Call - Method: {}, Params: {:?}", method, params);
        self.provider
            .request(method.clone(), params)
            .await
            .inspect(|res| log::trace!("RPC Response: {:?}", res))
            .map_err(|e| match e {
                RpcError::Original(AlloyError::ErrorResp(err)) => {
                    anyhow!("Error response from RPC service: {err}")
                }
                RpcError::Original(AlloyError::SerError(err)) => {
                    anyhow!("Serialization error: {err}")
                }
                RpcError::Original(AlloyError::DeserError { err, text }) => {
                    log::debug!("Deserialization error: {err}, response text: {text}");
                    anyhow!("Deserialization error: {err}")
                }
                _ => anyhow!("{e}"),
            })
            .map_err(|e| Error::RpcRequestError(e.to_string()))
    }

    /// Gets the total count of entities in GolemBase.
    /// Returns the number of entities currently stored.
    pub async fn get_entity_count(&self) -> Result<u64, Error> {
        self.rpc_call::<(), u64>("arkiv_getEntityCount", ()).await
    }

    /// Gets the entity keys of all entities in GolemBase.
    /// Returns a vector of all entity keys.
    pub async fn get_all_entity_keys(&self) -> Result<Vec<Hash>, Error> {
        let result = self
            .rpc_call::<(), Option<Vec<Hash>>>("arkiv_getAllEntityKeys", ())
            .await?;
        Ok(result.unwrap_or_default())
    }

    /// Gets the entity keys of all entities owned by the given address.
    /// Returns a vector of entity keys for the specified owner.
    pub async fn get_entities_of_owner(&self, address: Address) -> Result<Vec<Hash>, Error> {
        let result = self
            .rpc_call::<&[Address], Option<Vec<Hash>>>("arkiv_getEntitiesOfOwner", &[address])
            .await?;
        Ok(result.unwrap_or_default())
    }

    /// Gets the storage value associated with the given entity key.
    /// Decodes the value from base64 and attempts to convert it to the requested type.
    pub async fn get_storage_value<T: TryFrom<Vec<u8>>>(&self, key: Hash) -> Result<T, Error>
    where
        <T as TryFrom<Vec<u8>>>::Error: std::fmt::Display,
    {
        let query = format!("$key = {}", key);
        let options = QueryOptions::empty().with_payload();
        let search = self.query_with_options(&query, &options).await?;

        if search.is_empty() {
            return Err(Error::UnexpectedError("No search result found".to_string()));
        }

        if search.len() > 1 {
            log::warn!("Multiple entities found for key {key}, returning the first one");
        }

        let value = search[0]
            .value
            .as_ref()
            .map(|value| value.to_vec())
            .ok_or(Error::UnexpectedError("Value is not present".to_string()))?;
        T::try_from(value).map_err(|e| Error::UnexpectedError(e.to_string()))
    }

    /// Queries entities in GolemBase based on annotations with custom options.
    /// Returns a vector of `SearchResult` matching the query string and options.
    pub async fn query_with_options(
        &self,
        query: &str,
        options: &QueryOptions,
    ) -> Result<Vec<SearchResult>, Error> {
        let results = self
            .rpc_call::<(&str, &QueryOptions), Vec<SearchResult>>(
                "arkiv_queryEntities",
                (&query, options),
            )
            .await?;
        Ok(results)
    }

    /// Queries entities in GolemBase based on annotations.
    /// Returns a vector of `SearchResult` matching the query string.
    pub async fn query_entities(&self, query: &str) -> Result<Vec<SearchResult>, Error> {
        self.query_with_options(query, &QueryOptions::with_all())
            .await
    }

    /// Queries entities in GolemBase based on annotations and returns only their keys.
    /// Returns a vector of entity keys matching the query string.
    pub async fn query_entity_keys(&self, query: &str) -> Result<Vec<Hash>, Error> {
        self.query_with_options(query, &QueryOptions::empty().with_key())
            .await
            .map(|results| results.into_iter().map(|result| result.key).collect())
    }

    /// Gets all entity keys for entities that will expire at the given block number.
    /// Returns a vector of entity keys expiring at the specified block.
    pub async fn get_entities_to_expire_at_block(
        &self,
        block_number: u64,
    ) -> Result<Vec<Hash>, Error> {
        let result = self
            .rpc_call::<u64, Option<Vec<Hash>>>("arkiv_getEntitiesToExpireAtBlock", block_number)
            .await?;
        Ok(result.unwrap_or_default())
    }

    /// Gets metadata for a specific entity.
    /// Returns an `EntityMetaData` struct for the given entity key.
    pub async fn get_entity_metadata(&self, key: Hash) -> Result<EntityMetaData, Error> {
        self.rpc_call::<&[Hash], EntityMetaData>("arkiv_getEntityMetaData", &[key])
            .await
    }
}
