use alloy::primitives::{Address, U160};
use alloy::rpc::json_rpc::{RpcRecv, RpcSend};
use alloy_json_rpc::RpcError as AlloyError;
use anyhow::anyhow;
use bytes::Bytes;
use displaydoc::Display;
use futures::stream::{self, Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt::Debug;
use thiserror::Error;

use crate::resilient_provider::RpcError;
use crate::{ArkivClient, Hash, NumericAnnotation, StringAnnotation};

/// Represents errors that can occur in the Arkiv RPC module.
/// Used to wrap and describe errors from RPC requests, decoding, or deserialization.
#[derive(Debug, Display, Error)]
pub enum Error {
    /// Failed to send the RPC request: {0}
    RpcRequestError(String),
    /// Failed to decode the hex-encoded storage value: {0}
    HexDecodeError(String),
    /// Failed to deserialize the RPC response: {0}
    ResponseDeserializationError(String),
    /// Unexpected error occurred: {0}
    UnexpectedError(String),
}

/// Represents a single search result from a query.
/// Contains the entity key, value (decoded from hex), expiration, owner, and annotations.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchResult {
    #[serde(rename = "key")]
    pub key: Hash,
    #[serde(
        rename = "value",
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_hex",
        serialize_with = "serialize_optional_hex",
        default
    )]
    pub value: Option<Bytes>,
    #[serde(rename = "contentType", skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(rename = "expiresAt", skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
    #[serde(rename = "owner", skip_serializing_if = "Option::is_none")]
    pub owner: Option<Address>,
    #[serde(
        rename = "lastModifiedAtBlock",
        skip_serializing_if = "Option::is_none"
    )]
    pub last_modified_at_block: Option<u64>,
    #[serde(
        rename = "transactionIndexInBlock",
        skip_serializing_if = "Option::is_none"
    )]
    pub transaction_index_in_block: Option<u64>,
    #[serde(
        rename = "operationIndexInTransaction",
        skip_serializing_if = "Option::is_none"
    )]
    pub operation_index_in_transaction: Option<u64>,
    #[serde(
        rename = "stringAttributes",
        skip_serializing_if = "Vec::is_empty",
        default
    )]
    pub string_annotations: Vec<StringAnnotation>,
    #[serde(
        rename = "numericAttributes",
        skip_serializing_if = "Vec::is_empty",
        default
    )]
    pub numeric_annotations: Vec<NumericAnnotation>,
}

/// Specifies how to order query results.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderByAnnotation {
    /// The annotation key to order by
    #[serde(rename = "key")]
    pub key: String,
    /// Whether to order in ascending order (true) or descending order (false)
    #[serde(rename = "ascending")]
    pub ascending: bool,
}

/// Controls what data to include in query results.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IncludeData {
    /// Include the key field
    #[serde(rename = "key")]
    pub key: bool,
    /// Include attributes (string and numeric annotations)
    #[serde(rename = "attributes")]
    pub attributes: bool,
    /// Include the payload data
    #[serde(rename = "payload")]
    pub payload: bool,
    /// Include content type information
    #[serde(rename = "contentType")]
    pub content_type: bool,
    /// Include expiration information
    #[serde(rename = "expiration")]
    pub expiration: bool,
    /// Include owner information
    #[serde(rename = "owner")]
    pub owner: bool,
    /// Include last modified at block information
    #[serde(rename = "lastModifiedAtBlock")]
    pub last_modified_at_block: bool,
    /// Include transaction index in block information
    #[serde(rename = "transactionIndexInBlock")]
    pub transaction_index_in_block: bool,
    /// Include operation index in transaction information
    #[serde(rename = "operationIndexInTransaction")]
    pub operation_index_in_transaction: bool,
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

impl QueryResponse {
    /// Normalizes the QueryResponse by normalizing all SearchResults in the data.
    /// This ensures that zero addresses are converted to None in all search results.
    pub fn normalize(self) -> Self {
        Self {
            data: self
                .data
                .into_iter()
                .map(|result| result.normalize())
                .collect(),
            ..self
        }
    }
}

/// Options for querying entities in Arkiv.
/// Controls pagination, data inclusion, and block number for queries.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueryOptions {
    /// The block number at which to query entities.
    #[serde(rename = "atBlock", skip_serializing_if = "Option::is_none")]
    pub at_block: Option<u64>,
    /// Controls what data to include in the results.
    #[serde(rename = "includeData", skip_serializing_if = "Option::is_none")]
    pub include_data: Option<IncludeData>,
    /// Ordering specification for query results.
    #[serde(rename = "orderBy", skip_serializing_if = "Vec::is_empty", default)]
    pub order_by: Vec<OrderByAnnotation>,
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
            attributes: true,
            payload: true,
            content_type: true,
            expiration: true,
            owner: true,
            last_modified_at_block: true,
            transaction_index_in_block: true,
            operation_index_in_transaction: true,
        }
    }

    /// Creates an `IncludeData` with only keys enabled.
    pub fn keys_only() -> Self {
        Self {
            key: true,
            attributes: false,
            payload: false,
            content_type: false,
            expiration: false,
            owner: false,
            last_modified_at_block: false,
            transaction_index_in_block: false,
            operation_index_in_transaction: false,
        }
    }

    /// Creates an `IncludeData` with metadata only (no payload).
    pub fn metadata_only() -> Self {
        Self {
            key: true,
            attributes: true,
            payload: false,
            content_type: true,
            expiration: true,
            owner: true,
            last_modified_at_block: true,
            transaction_index_in_block: true,
            operation_index_in_transaction: true,
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
            order_by: Vec::new(),
            results_per_page: crate::client::DEFAULT_RESULTS_PER_PAGE,
            cursor: None,
        }
    }

    /// Creates a `QueryOptions` with all columns selected and attributes enabled.
    pub fn with_all() -> Self {
        Self {
            at_block: None,
            include_data: Some(IncludeData::all()),
            order_by: Vec::new(),
            results_per_page: crate::client::DEFAULT_RESULTS_PER_PAGE,
            cursor: None,
        }
    }

    /// Sets the page size for pagination.
    pub fn with_page_size(mut self, page_size: u64) -> Self {
        self.results_per_page = page_size;
        self
    }

    /// Sets the block number at which to query entities.
    pub fn at_block(mut self, at_block: u64) -> Self {
        self.at_block = Some(at_block);
        self
    }

    /// Sets whether to include attributes (annotations) in query results.
    pub fn with_annotations(mut self, include_annotations: bool) -> Self {
        if let Some(ref mut data) = self.include_data {
            data.attributes = include_annotations;
        }
        self
    }

    /// Sets the ordering for query results.
    pub fn order_by(mut self, order_by: Vec<OrderByAnnotation>) -> Self {
        self.order_by = order_by;
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

/// Helper function to decode a hex string into Bytes
/// Handles both prefixed (0x...) and non-prefixed hex strings
fn decode_hex_string<'de, D>(s: &str) -> Result<Bytes, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let hex_str = if s.starts_with("0x") { &s[2..] } else { s };
    hex::decode(hex_str)
        .map(Bytes::from)
        .map_err(serde::de::Error::custom)
}

/// Helper for deserializing hex-encoded storage values.
/// Handles both prefixed (0x...) and non-prefixed hex strings.
/// Used to decode entity values returned from the RPC API.
pub fn deserialize_hex<'de, D>(deserializer: D) -> Result<Bytes, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    decode_hex_string::<D>(&s)
}

/// Encode bytes as hex string with 0x prefix
pub fn encode_prefixed_hex(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}

/// Serialize Bytes as hex string with 0x prefix
pub fn serialize_hex<S>(value: &Bytes, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&encode_prefixed_hex(value))
}

/// Helper for deserializing optional hex-encoded storage values.
/// Handles both prefixed (0x...) and non-prefixed hex strings.
/// Used to decode optional entity values returned from the RPC API.
pub fn deserialize_optional_hex<'de, D>(deserializer: D) -> Result<Option<Bytes>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)?
        .map(|str| decode_hex_string::<D>(&str))
        .transpose()
}

/// Serialize optional Bytes as hex string with 0x prefix
pub fn serialize_optional_hex<S>(value: &Option<Bytes>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match value {
        Some(bytes) => serialize_hex(bytes, serializer),
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

    /// Normalizes the SearchResult by converting zero addresses to None.
    /// Arkiv returns owner as 0x0 as a default value when we didn't ask for address.
    /// This method replaces it with proper None to not mislead users.
    pub fn normalize(self) -> Self {
        Self {
            owner: self
                .owner
                .map(|owner| match owner {
                    addr if addr == Address::from(U160::ZERO) => None,
                    _ => Some(owner),
                })
                .flatten(),
            ..self
        }
    }

    /// Finds a string annotation by its key.
    /// Returns `Some(annotation)` if found, `None` otherwise.
    pub fn find_string_annotation(&self, key: &str) -> Option<&StringAnnotation> {
        self.string_annotations.iter().find(|a| a.key == key)
    }

    /// Finds a numeric annotation by its key.
    /// Returns `Some(annotation)` if found, `None` otherwise.
    pub fn find_numeric_annotation(&self, key: &str) -> Option<&NumericAnnotation> {
        self.numeric_annotations.iter().find(|a| a.key == key)
    }
}

impl ArkivClient {
    /// Makes a JSON-RPC call to the Arkiv endpoint.
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

    /// Gets the total count of entities in Arkiv.
    /// Returns the number of entities currently stored.
    pub async fn get_entity_count(&self) -> Result<u64, Error> {
        self.rpc_call::<(), u64>("arkiv_getEntityCount", ()).await
    }

    /// Gets the entity keys of all entities in Arkiv.
    /// Returns a vector of all entity keys.
    pub async fn get_all_entity_keys(&self) -> Result<Vec<Hash>, Error> {
        // This is workaround to get all entity keys, because there is no RPC call for this.
        // Owner should never be 0x0, so it will return all entities.
        let query = format!("!($owner=0x0000000000000000000000000000000000000000)");
        let options = QueryOptions::empty().with_key();
        self.query_with_options(&query, &options)
            .await
            .map(|results| results.into_iter().map(|result| result.key).collect())
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
            return Err(Error::UnexpectedError(format!(
                "No entity found with the given key: {key}"
            )));
        }

        if search.len() > 1 {
            log::warn!("Multiple entities found for key {key}, returning the first one");
        }

        Ok(search[0].clone())
    }

    /// Gets the storage value associated with the given entity key.
    /// Decodes the value from hex and attempts to convert it to the requested type.
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

    /// Queries entities in Arkiv based on annotations with custom options.
    /// Returns a vector of `SearchResult` matching the query string and options.
    /// Collects all results from the paginated stream, ignoring up to `max_query_errors` errors.
    pub async fn query_with_options(
        &self,
        query: &str,
        options: &QueryOptions,
    ) -> Result<Vec<SearchResult>, Error> {
        let max_errors = self.tx_config.max_query_errors;
        let mut error_count = 0u32;
        let mut all_results = Vec::new();

        let mut stream = Box::pin(self.query_streamed(query, options));

        while let Some(result) = stream.next().await {
            match result {
                Ok(mut results) => {
                    all_results.append(&mut results);
                }
                Err(e) => {
                    error_count += 1;
                    if error_count > max_errors {
                        return Err(Error::UnexpectedError(format!(
                            "Too many query errors: {error_count} (max allowed: {max_errors}) - Last error: {e}"
                        )));
                    }
                    log::warn!("Query error ({error_count} of {max_errors}): {e}");
                }
            }
        }

        Ok(all_results)
    }

    /// Queries entities in Arkiv based on annotations.
    /// Returns a vector of `SearchResult` matching the query string.
    pub async fn query_entities(&self, query: &str) -> Result<Vec<SearchResult>, Error> {
        self.query_with_options(query, &QueryOptions::with_all())
            .await
    }

    /// Queries entities in Arkiv based on annotations and returns only their keys.
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

    /// Queries entities in Arkiv with pagination support, returning a stream of results.
    /// Automatically handles cursor-based pagination by making subsequent RPC calls.
    /// If results_per_page is 0, uses the default value from the client's TransactionConfig,
    /// falling back to `DEFAULT_RESULTS_PER_PAGE` constant if config is not available.
    pub fn query_streamed(
        &self,
        query: &str,
        options: &QueryOptions,
    ) -> impl Stream<Item = Result<Vec<SearchResult>, Error>> + '_ {
        let query = query.to_string();
        let mut options = options.clone();

        // Set default results per page if not specified
        if options.results_per_page == 0 {
            options.results_per_page = self.tx_config.default_results_per_page;
        }

        stream::unfold(Some(options), move |state| {
            let client = self;
            let query = query.clone();

            async move {
                let mut options = match state {
                    Some(state) => state,
                    // If no cursor was returned in previous iteration, we've reached the end of the stream.
                    None => return None,
                };

                log::trace!("Querying entities with query: {query}, options: {options:?}");
                let response = client
                    .rpc_call::<(&str, &QueryOptions), QueryResponse>(
                        "arkiv_query",
                        (&query, &options),
                    )
                    .await;

                log::trace!("Received query response: {response:?}");
                match response {
                    Ok(response) => {
                        let response = response.normalize();
                        let data = response.data;

                        // Ensure that we query state from the same block even if user didn't
                        // provide any specific block number.
                        options.at_block = Some(response.block_number);

                        match response.cursor {
                            Some(cursor) => {
                                // Update cursor as new starting position for the query in next iteration.
                                let next_options = QueryOptions {
                                    cursor: Some(cursor),
                                    ..options
                                };
                                Some((Ok(data), Some(next_options)))
                            }
                            // If cursor is None, we've reached the end of the stream and we will return
                            // None to end the stream in next iteration.
                            None => Some((Ok(data), None)),
                        }
                    }
                    Err(e) => Some((Err(e), Some(options))),
                }
            }
        })
    }
}
