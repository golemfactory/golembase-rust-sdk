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

/// Represents a single search result from a query.
/// Contains the entity key, value (decoded from base64), expiration, owner, and annotations.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchResult {
    #[serde(rename = "key")]
    pub key: Hash,
    #[serde(rename = "value", skip_serializing_if = "Option::is_none")]
    pub value: Option<Bytes>,
    #[serde(rename = "expiresAt", skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
    #[serde(rename = "owner", skip_serializing_if = "Option::is_none")]
    pub owner: Option<Address>,
    #[serde(rename = "stringAnnotations", skip_serializing_if = "Vec::is_empty")]
    pub string_annotations: Vec<StringAnnotation>,
    #[serde(rename = "numericAnnotations", skip_serializing_if = "Vec::is_empty")]
    pub numeric_annotations: Vec<NumericAnnotation>,
}

/// Controls what data to include in query results.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IncludeData {
    /// Include the key field
    #[serde(rename = "key")]
    pub key: bool,
    /// Include annotations (string and numeric)
    #[serde(rename = "annotations")]
    pub annotations: bool,
    /// Include the payload data
    #[serde(rename = "payload")]
    pub payload: bool,
    /// Include expiration information
    #[serde(rename = "expiration")]
    pub expiration: bool,
    /// Include owner information
    #[serde(rename = "owner")]
    pub owner: bool,
}

/// Response structure for query operations.
/// Wraps the search results with metadata including block number and pagination cursor.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueryResponse {
    #[serde(rename = "data")]
    pub data: Vec<SearchResult>,
    #[serde(rename = "blockNumber")]
    pub block_number: u64,
    #[serde(rename = "cursor", skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

/// Options for querying entities in GolemBase.
/// Controls pagination, data inclusion, and block number for queries.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueryOptions {
    /// The block number at which to query entities.
    #[serde(rename = "atBlock", skip_serializing_if = "Option::is_none")]
    pub at_block: Option<u64>,
    /// Controls what data to include in the results.
    #[serde(rename = "includeData", skip_serializing_if = "Option::is_none")]
    pub include_data: Option<IncludeData>,
    /// Maximum number of results per page.
    #[serde(rename = "resultsPerPage")]
    pub results_per_page: u64,
    /// Cursor for pagination (opaque string).
    #[serde(rename = "cursor", skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

impl Default for QueryOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl IncludeData {
    /// Creates an `IncludeData` with all fields enabled.
    pub fn all() -> Self {
        Self {
            key: true,
            annotations: true,
            payload: true,
            expiration: true,
            owner: true,
        }
    }

    /// Creates an `IncludeData` with only keys enabled.
    pub fn keys_only() -> Self {
        Self {
            key: true,
            annotations: false,
            payload: false,
            expiration: false,
            owner: false,
        }
    }

    /// Creates an `IncludeData` with metadata only (no payload).
    pub fn metadata_only() -> Self {
        Self {
            key: true,
            annotations: true,
            payload: false,
            expiration: true,
            owner: true,
        }
    }
}

impl QueryOptions {
    /// Creates a new `QueryOptions` with default settings.
    pub fn new() -> Self {
        Self::empty()
    }

    /// Creates an empty `QueryOptions` with no columns selected.
    fn empty() -> Self {
        Self {
            at_block: None,
            include_data: Some(IncludeData::keys_only()),
            results_per_page: 100,
            cursor: None,
        }
    }

    /// Creates a `QueryOptions` with all columns selected and annotations enabled.
    pub fn with_all() -> Self {
        Self {
            at_block: None,
            include_data: Some(IncludeData::all()),
            results_per_page: 100,
            cursor: None,
        }
    }

    /// Sets the block number at which to query entities.
    pub fn at_block(mut self, at_block: u64) -> Self {
        self.at_block = Some(at_block);
        self
    }

    /// Sets whether to include annotations in query results.
    pub fn with_annotations(mut self, include_annotations: bool) -> Self {
        if let Some(ref mut data) = self.include_data {
            data.annotations = include_annotations;
        }
        self
    }

    /// Includes the key column in query results.
    pub fn with_key(mut self) -> Self {
        if let Some(ref mut data) = self.include_data {
            data.key = true;
        }
        self
    }

    /// Includes the payload column in query results.
    pub fn with_payload(mut self) -> Self {
        if let Some(ref mut data) = self.include_data {
            data.payload = true;
        }
        self
    }

    /// Includes the expires_at column in query results.
    pub fn with_expires_at(mut self) -> Self {
        if let Some(ref mut data) = self.include_data {
            data.expiration = true;
        }
        self
    }

    /// Includes the owner_address column in query results.
    pub fn with_owner_address(mut self) -> Self {
        if let Some(ref mut data) = self.include_data {
            data.owner = true;
        }
        self
    }

    /// Excludes the key column from query results.
    pub fn exclude_key(mut self) -> Self {
        if let Some(ref mut data) = self.include_data {
            data.key = false;
        }
        self
    }

    /// Excludes the payload column from query results.
    pub fn exclude_payload(mut self) -> Self {
        if let Some(ref mut data) = self.include_data {
            data.payload = false;
        }
        self
    }

    /// Excludes the expires_at column from query results.
    pub fn exclude_expires_at(mut self) -> Self {
        if let Some(ref mut data) = self.include_data {
            data.expiration = false;
        }
        self
    }

    /// Excludes the owner_address column from query results.
    pub fn exclude_owner_address(mut self) -> Self {
        if let Some(ref mut data) = self.include_data {
            data.owner = false;
        }
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
        let query = format!("$owner = {}", address);
        let options = QueryOptions::empty().with_key();
        self.query_with_options(&query, &options)
            .await
            .map(|results| results.into_iter().map(|result| result.key).collect())
    }

    /// Gets an entity by its key with all metadata.
    /// Returns the first matching SearchResult or an error if not found.
    async fn get_entity(&self, key: Hash) -> Result<SearchResult, Error> {
        let query = format!("$key = {}", key);
        let options = QueryOptions::with_all();
        let search = self.query_with_options(&query, &options).await?;

        if search.is_empty() {
            return Err(Error::UnexpectedError(
                "No entity found with the given key".to_string(),
            ));
        }

        if search.len() > 1 {
            log::warn!("Multiple entities found for key {key}, returning the first one");
        }

        Ok(search[0].clone())
    }

    /// Gets the storage value associated with the given entity key.
    /// Decodes the value from base64 and attempts to convert it to the requested type.
    pub async fn get_storage_value<T: TryFrom<Vec<u8>>>(&self, key: Hash) -> Result<T, Error>
    where
        <T as TryFrom<Vec<u8>>>::Error: std::fmt::Display,
    {
        let search_result = self.get_entity(key).await?;
        let value = search_result
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
        let response = self
            .rpc_call::<(&str, &QueryOptions), QueryResponse>("arkiv_query", (&query, options))
            .await?;
        Ok(response.data)
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
        // Query entities that expire at the specified block number
        let query = format!("$expiration = {}", block_number);
        let options = QueryOptions::empty().with_key();

        self.query_with_options(&query, &options)
            .await
            .map(|results| results.into_iter().map(|result| result.key).collect())
    }

    /// Gets the metadata for a specific entity by its key.
    /// Returns a `SearchResult` containing the entity's metadata including annotations, owner, and expiration.
    pub async fn get_entity_metadata(&self, key: Hash) -> Result<SearchResult, Error> {
        self.get_entity(key).await
    }
}
