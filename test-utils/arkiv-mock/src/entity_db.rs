use alloy::primitives::{keccak256, Address, B256};
use anyhow::{anyhow, Result};
use arkiv_sdk::entity::{Create, NumericAnnotation, StringAnnotation, Update};
use arkiv_sdk::rpc::{serialize_hex, SearchResult};
use arkiv_sdk::rpc::{IncludeData, QueryOptions};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::string::FromUtf8Error;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::query_parser::{Expression, Parser, QueryCondition};

/// Represents a GolemBase entity
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entity {
    #[serde(serialize_with = "serialize_b256")]
    pub key: B256,
    #[serde(serialize_with = "serialize_hex")]
    pub data: Bytes,
    pub btl: u64,
    #[serde(serialize_with = "serialize_address")]
    pub owner: Address,
    pub expires_at: Option<u64>,
    pub string_annotations: Vec<StringAnnotation>,
    pub numeric_annotations: Vec<NumericAnnotation>,
}

impl Entity {
    /// Create a new entity from a GolemBase Create object and owner
    pub fn create(create: Create, owner: Address) -> Self {
        let entity = Self {
            key: B256::ZERO, // Temporary key, will be calculated below
            data: create.data.clone(),
            btl: create.btl,
            owner,
            expires_at: None, // Will be computed based on current block + BTL
            string_annotations: create.string_annotations,
            numeric_annotations: create.numeric_annotations,
        };

        entity
    }

    /// Calculate the hash of this entity based on its content, owner, block number, index and transaction hash
    /// This creates a deterministic identifier that combines entity content with block context
    pub fn calculate_hash(&self, block_number: u64, idx: usize, transaction_hash: B256) -> B256 {
        let mut content_bytes = Vec::new();
        content_bytes.extend_from_slice(&self.data);
        content_bytes.extend_from_slice(self.btl.to_le_bytes().as_slice());
        content_bytes.extend_from_slice(self.owner.as_slice());
        content_bytes.extend_from_slice(block_number.to_le_bytes().as_slice());
        content_bytes.extend_from_slice(idx.to_le_bytes().as_slice());
        content_bytes.extend_from_slice(transaction_hash.as_slice());

        // Annotations don't need to be part of hash, because it will be unique anyway.

        keccak256(&content_bytes)
    }

    /// Set the entity key to a hash based on block number, index and transaction hash
    /// Also sets the expiration block number based on current block + BTL
    /// This modifies the entity in place and returns self for chaining
    pub fn with_hash(mut self, block_number: u64, idx: usize, transaction_hash: B256) -> Self {
        self.key = self.calculate_hash(block_number, idx, transaction_hash);
        self.expires_at = Some(block_number + self.btl);
        self
    }

    /// Update this entity with data from a GolemBase Update object
    /// This modifies the entity in place and returns self for chaining
    pub fn update(&mut self, update: &Update) -> &mut Self {
        self.data = update.data.clone();
        self.btl = update.btl;
        self.string_annotations = update.string_annotations.clone();
        self.numeric_annotations = update.numeric_annotations.clone();
        self
    }

    /// Check if this entity matches a specific query condition
    pub fn matches_condition(&self, condition: &QueryCondition) -> bool {
        match condition {
            QueryCondition::StringEquals(key, value) => {
                // Check if the entity has this string annotation key with the specified value
                self.string_annotations
                    .iter()
                    .find(|a| a.key == *key)
                    .map_or(false, |a| a.value == *value)
            }
            QueryCondition::StringNotEquals(key, value) => {
                // Check if the entity has this string annotation key with a different value
                self.string_annotations
                    .iter()
                    .find(|a| a.key == *key)
                    .map_or(true, |a| a.value != *value)
            }
            QueryCondition::NumericEquals(key, value) => {
                // Check if the entity has this numeric annotation key with the specified value
                self.numeric_annotations
                    .iter()
                    .find(|a| a.key == *key)
                    .map_or(false, |a| a.value == *value)
            }
            QueryCondition::NumericNotEquals(key, value) => {
                // Check if the entity has this numeric annotation key with a different value
                self.numeric_annotations
                    .iter()
                    .find(|a| a.key == *key)
                    .map_or(true, |a| a.value != *value)
            }
            QueryCondition::NumericLessThan(key, value) => {
                // Check if the entity has this numeric annotation key with a value less than specified
                self.numeric_annotations
                    .iter()
                    .find(|a| a.key == *key)
                    .map_or(false, |a| a.value < *value)
            }
            QueryCondition::NumericGreaterThan(key, value) => {
                // Check if the entity has this numeric annotation key with a value greater than specified
                self.numeric_annotations
                    .iter()
                    .find(|a| a.key == *key)
                    .map_or(false, |a| a.value > *value)
            }
            QueryCondition::NumericLessThanOrEqual(key, value) => {
                // Check if the entity has this numeric annotation key with a value less than or equal to specified
                self.numeric_annotations
                    .iter()
                    .find(|a| a.key == *key)
                    .map_or(false, |a| a.value <= *value)
            }
            QueryCondition::NumericGreaterThanOrEqual(key, value) => {
                // Check if the entity has this numeric annotation key with a value greater than or equal to specified
                self.numeric_annotations
                    .iter()
                    .find(|a| a.key == *key)
                    .map_or(false, |a| a.value >= *value)
            }
            QueryCondition::OwnerEquals(value) => {
                // Compare the entity's owner with the specified address
                self.owner == *value
            }
            QueryCondition::KeyEquals(value) => {
                // Compare the entity's key with the specified key
                self.key == *value
            }
            QueryCondition::ExpirationEquals(value) => {
                // Compare the entity's expires_at with the specified block number
                self.expires_at == Some(*value)
            }
        }
    }

    pub fn to_search_result(&self, options: &QueryOptions) -> SearchResult {
        let include_data = options.clone().include_data.unwrap_or(IncludeData::all());

        SearchResult {
            key: match include_data.key {
                true => self.key,
                false => Default::default(),
            },
            value: match include_data.payload {
                true => Some(self.data.clone()),
                false => None,
            },
            expires_at: match include_data.expiration {
                true => self.expires_at,
                false => None,
            },
            owner: match include_data.owner {
                true => Some(self.owner),
                false => None,
            },
            string_annotations: match include_data.attributes {
                true => self.string_annotations.clone(),
                false => Vec::new(),
            },
            numeric_annotations: match include_data.attributes {
                true => self.numeric_annotations.clone(),
                false => Vec::new(),
            },
        }
    }
}

impl TryFrom<Entity> for arkiv_sdk::entity::Entity {
    type Error = FromUtf8Error;

    fn try_from(local_entity: Entity) -> Result<Self, Self::Error> {
        Ok(Self {
            data: String::from_utf8(local_entity.data.to_vec())?,
            btl: local_entity.btl,
            string_annotations: local_entity.string_annotations,
            numeric_annotations: local_entity.numeric_annotations,
        })
    }
}

impl From<&Entity> for SearchResult {
    fn from(entity: &Entity) -> Self {
        Self {
            key: entity.key,
            value: Some(entity.data.clone()),
            expires_at: Some(entity.expires_at.unwrap_or(0)),
            owner: Some(entity.owner),
            string_annotations: entity.string_annotations.clone(),
            numeric_annotations: entity.numeric_annotations.clone(),
        }
    }
}

/// Internal state of the entity database
#[derive(Clone, Debug, Default)]
struct EntityDbState {
    entities: HashMap<B256, Entity>,
    string_annotations: HashMap<String, Vec<B256>>,
    numeric_annotations: HashMap<String, Vec<B256>>, // Indexed by key, not value
    entities_by_owner: HashMap<Address, Vec<B256>>,  // Index entities by owner
}

impl EntityDbState {
    /// Evaluate a parsed expression to get matching entity keys
    fn evaluate_expression(&self, expression: &Expression) -> Vec<B256> {
        match expression {
            Expression::Condition(condition) => {
                // Use annotation maps to find entity keys, then filter by actual annotation values
                self.get_candidates(condition)
                    .into_iter()
                    .filter_map(|key| {
                        self.entities.get(&key).and_then(|entity| {
                            if entity.matches_condition(condition) {
                                Some(key)
                            } else {
                                None
                            }
                        })
                    })
                    .collect()
            }
            Expression::And(left, right) => {
                let left_keys: HashSet<B256> = self.evaluate_expression(left).into_iter().collect();
                let right_keys: HashSet<B256> =
                    self.evaluate_expression(right).into_iter().collect();
                left_keys.intersection(&right_keys).cloned().collect()
            }
            Expression::Or(left, right) => {
                let left_keys: HashSet<B256> = self.evaluate_expression(left).into_iter().collect();
                let right_keys: HashSet<B256> =
                    self.evaluate_expression(right).into_iter().collect();
                left_keys.union(&right_keys).cloned().collect()
            }
            Expression::Not(expr) => {
                // Get all entity keys
                let all_keys: HashSet<B256> = self.entities.keys().cloned().collect();
                // Get keys that match the negated expression
                let matching_keys: HashSet<B256> =
                    self.evaluate_expression(expr).into_iter().collect();
                // Return keys that are NOT in the matching set
                all_keys.difference(&matching_keys).cloned().collect()
            }
        }
    }

    /// Get candidate entity keys from annotation maps for a given condition
    fn get_candidates(&self, condition: &QueryCondition) -> Vec<B256> {
        match condition {
            QueryCondition::StringEquals(key, _) | QueryCondition::StringNotEquals(key, _) => {
                // Find entities that have this string annotation key
                self.string_annotations
                    .get(key)
                    .cloned()
                    .unwrap_or_default()
            }
            QueryCondition::NumericEquals(key, _)
            | QueryCondition::NumericNotEquals(key, _)
            | QueryCondition::NumericLessThan(key, _)
            | QueryCondition::NumericGreaterThan(key, _)
            | QueryCondition::NumericLessThanOrEqual(key, _)
            | QueryCondition::NumericGreaterThanOrEqual(key, _) => {
                // Find entities that have this numeric annotation key
                self.numeric_annotations
                    .get(key)
                    .cloned()
                    .unwrap_or_default()
            }
            QueryCondition::OwnerEquals(value) => {
                // Find entities by owner address
                self.entities_by_owner
                    .get(value)
                    .cloned()
                    .unwrap_or_default()
            }
            QueryCondition::KeyEquals(value) => {
                // Find entity by key
                if self.entities.contains_key(value) {
                    vec![*value]
                } else {
                    vec![]
                }
            }
            QueryCondition::ExpirationEquals(value) => {
                // Find entities that expire at the specified block number
                self.entities
                    .values()
                    .filter_map(|entity| {
                        entity
                            .expires_at
                            .filter(|&expires_at| expires_at == *value)
                            .map(|_| entity.key)
                    })
                    .collect()
            }
        }
    }

    /// Query entity keys by parsing a query string and finding keys matching all specified annotations
    /// Query format: Supports &&, ||, parentheses, and meta-annotations
    /// Examples: "test_type = \"Test\"", "priority = 1", "tag = \"important\" && priority = 1"
    fn query_entity_keys(&self, query: &str) -> Result<Vec<B256>> {
        // Parse query string to extract conditions
        let expression =
            Parser::parse_query(query).map_err(|e| anyhow!("Failed to parse query: {}", e))?;

        log::trace!("Parsed expression: {:?}", expression);

        // Evaluate the expression to get matching entity keys
        Ok(self.evaluate_expression(&expression))
    }

    /// Query entities by parsing a query string and finding entities matching all specified annotations
    /// Query format: Supports &&, ||, parentheses, and meta-annotations
    /// Examples: "test_type = \"Test\"", "priority = 1", "tag = \"important\" && priority = 1"
    fn query_entities(&self, query: &str) -> Result<Vec<Entity>> {
        let matching_keys = self.query_entity_keys(query)?;

        // Return entities for matching keys
        let mut entities = Vec::new();
        for key in matching_keys {
            if let Some(entity) = self.entities.get(&key) {
                entities.push(entity.clone());
            }
        }

        Ok(entities)
    }
}

/// GolemBase entity database
#[derive(Clone, Debug, Default)]
pub struct EntityDb {
    state: Arc<RwLock<EntityDbState>>,
}

impl EntityDb {
    /// Create a new empty entity database
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(EntityDbState {
                entities: HashMap::new(),
                string_annotations: HashMap::new(),
                numeric_annotations: HashMap::new(),
                entities_by_owner: HashMap::new(),
            })),
        }
    }

    /// Add an entity to the database
    pub async fn add_entity(&self, entity: Entity) {
        let key = entity.key;
        let mut state = self.state.write().await;

        log::info!("Creating entity: key={}", key);
        // Add to main entities map
        state.entities.insert(key, entity.clone());

        // Update owner index
        state
            .entities_by_owner
            .entry(entity.owner)
            .or_insert_with(Vec::new)
            .push(entity.key);

        // Update annotation indices
        Self::update_annotations(
            &mut state,
            entity.key,
            &entity.string_annotations,
            &entity.numeric_annotations,
        );
    }

    /// Update an existing entity in the database with new data
    /// Returns true if entity was updated, false if entity doesn't exist
    pub async fn update_entity(&self, entity_key: &B256, update: &Update) -> bool {
        let mut state = self.state.write().await;

        if let Some(entity) = state.entities.get_mut(entity_key) {
            log::info!("Updating entity: key={}", entity_key);
            // Update the entity using the existing update method
            entity.update(update);

            // Get entity data to avoid borrowing conflicts
            let entity_key = entity.key;
            let new_string_annotations = entity.string_annotations.clone();
            let new_numeric_annotations = entity.numeric_annotations.clone();

            // Update annotation indices
            Self::update_annotations(
                &mut state,
                entity_key,
                &new_string_annotations,
                &new_numeric_annotations,
            );

            true
        } else {
            // Entity doesn't exist, ignore the update
            false
        }
    }

    /// Update only the BTL of an existing entity
    pub async fn update_entity_btl(&self, entity_key: &B256, new_btl: u64) {
        let mut state = self.state.write().await;

        if let Some(entity) = state.entities.get_mut(entity_key) {
            entity.btl = new_btl;
        }
    }

    /// Function to remove all annotations for a specific entity
    fn remove_entity_annotations(state: &mut EntityDbState, entity_key: B256) {
        // Remove all existing annotations for this entity from hash maps
        for (_, keys) in state.string_annotations.iter_mut() {
            keys.retain(|&k| k != entity_key);
        }
        for (_, keys) in state.numeric_annotations.iter_mut() {
            keys.retain(|&k| k != entity_key);
        }
    }

    /// Unified function to update annotation indices
    /// Removes all existing annotations for the entity and adds new ones
    fn update_annotations(
        state: &mut EntityDbState,
        entity_key: B256,
        new_string_annotations: &[StringAnnotation],
        new_numeric_annotations: &[NumericAnnotation],
    ) {
        // Remove all existing annotations first
        Self::remove_entity_annotations(state, entity_key);

        // Add new annotations to hash maps
        for annotation in new_string_annotations {
            state
                .string_annotations
                .entry(annotation.key.clone())
                .or_insert_with(Vec::new)
                .push(entity_key);
        }
        for annotation in new_numeric_annotations {
            state
                .numeric_annotations
                .entry(annotation.key.clone())
                .or_insert_with(Vec::new)
                .push(entity_key);
        }
    }

    /// Get an entity by its key
    pub async fn get_entity(&self, key: &B256) -> Option<Entity> {
        self.state.read().await.entities.get(key).cloned()
    }

    /// Get entities by string annotation
    pub async fn by_string_annotation(&self, annotation: &str) -> Vec<Entity> {
        let state = self.state.read().await;
        let keys = state
            .string_annotations
            .get(annotation)
            .cloned()
            .unwrap_or_default();
        let mut entities = Vec::new();

        for key in keys {
            if let Some(entity) = state.entities.get(&key) {
                entities.push(entity.clone());
            }
        }

        entities
    }

    /// Get entities by numeric annotation key
    pub async fn by_numeric_annotation_key(&self, annotation_key: &str) -> Vec<Entity> {
        let state = self.state.read().await;
        let keys = state
            .numeric_annotations
            .get(annotation_key)
            .cloned()
            .unwrap_or_default();
        let mut entities = Vec::new();

        for key in keys {
            if let Some(entity) = state.entities.get(&key) {
                entities.push(entity.clone());
            }
        }

        entities
    }

    /// Remove an entity from the database
    pub async fn remove_entity(&self, key: &B256) -> Option<Entity> {
        let mut state = self.state.write().await;
        if let Some(entity) = state.entities.remove(key) {
            log::info!("Removing entity: key={}, owner={}", key, entity.owner);
            // Remove from owner index
            if let Some(keys) = state.entities_by_owner.get_mut(&entity.owner) {
                keys.retain(|&k| k != entity.key);
            }

            // Remove all annotations for this entity
            Self::remove_entity_annotations(&mut state, entity.key);
            Some(entity)
        } else {
            None
        }
    }

    /// Get all entity keys
    pub async fn get_all_keys(&self) -> Vec<B256> {
        self.state.read().await.entities.keys().cloned().collect()
    }

    /// Get total number of entities
    pub async fn count(&self) -> usize {
        self.state.read().await.entities.len()
    }

    /// Get entities by owner address
    pub async fn get_entities_by_owner(&self, owner: &Address) -> Vec<B256> {
        self.state
            .read()
            .await
            .entities_by_owner
            .get(owner)
            .cloned()
            .unwrap_or_default()
    }

    /// Query entity keys by parsing a query string and finding keys matching all specified annotations
    /// Query format: Supports &&, ||, parentheses, and meta-annotations
    /// Examples: "test_type = \"Test\"", "priority = 1", "tag = \"important\" && priority = 1"
    pub async fn query_entity_keys(&self, query: &str) -> Result<Vec<B256>> {
        let state = self.state.read().await;
        state.query_entity_keys(query)
    }

    /// Query entities by parsing a query string and finding entities matching all specified annotations
    /// Query format: Supports &&, ||, parentheses, and meta-annotations
    /// Examples: "test_type = \"Test\"", "type = \"Test\"", "tag = \"important\" && priority = 1"
    pub async fn query_entities(&self, query: &str) -> Result<Vec<Entity>> {
        let state = self.state.read().await;
        state.query_entities(query)
    }

    /// Get entities that expire at the given block number
    pub async fn get_entities_expiring_at_block(&self, block_number: u64) -> Vec<B256> {
        let state = self.state.read().await;
        state
            .entities
            .values()
            .filter_map(|entity| {
                entity
                    .expires_at
                    .filter(|&expires_at| expires_at == block_number)
                    .map(|_| entity.key)
            })
            .collect()
    }

    /// Clear all entities from the database
    pub async fn clear_all(&self) -> usize {
        let mut state = self.state.write().await;
        let count = state.entities.len();
        state.entities.clear();
        state.string_annotations.clear();
        state.numeric_annotations.clear();
        state.entities_by_owner.clear();
        log::info!("Cleared all {} entities from database", count);
        count
    }
}

/// Serialize B256 as hex string with 0x prefix
fn serialize_b256<S>(value: &B256, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&format!("0x{:x}", value))
}

/// Serialize Address as hex string with 0x prefix
fn serialize_address<S>(value: &Address, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&format!("0x{:x}", value))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper function to populate an EntityDb with test entities
    /// Returns entity keys in order: [user_high_priority, admin, user_low_priority]
    async fn populate_test_entities(db: &EntityDb) -> [B256; 3] {
        // Create test owner addresses
        let owner1 = "0x1234567890123456789012345678901234567890"
            .parse::<Address>()
            .unwrap();
        let owner2 = "0x0987654321098765432109876543210987654321"
            .parse::<Address>()
            .unwrap();

        // Create entity 1: user with high priority
        let create1 = Create::new("user data 1".into(), 1000)
            .annotate_string("type", "user")
            .annotate_string("status", "active")
            .annotate_number("priority", 5u64)
            .annotate_number("age", 25u64);
        let entity1 = Entity::create(create1, owner1).with_hash(1, 0, B256::ZERO);
        let key1 = entity1.key;
        db.add_entity(entity1).await;

        // Create entity 2: admin with medium priority
        let create2 = Create::new("admin data".into(), 2000)
            .annotate_string("type", "admin")
            .annotate_string("status", "active")
            .annotate_number("priority", 3u64)
            .annotate_number("level", 10u64);
        let entity2 = Entity::create(create2, owner1).with_hash(1, 1, B256::ZERO);
        let key2 = entity2.key;
        db.add_entity(entity2).await;

        // Create entity 3: user with low priority, different owner
        let create3 = Create::new("user data 2".into(), 500)
            .annotate_string("type", "user")
            .annotate_string("status", "inactive")
            .annotate_number("priority", 1u64)
            .annotate_number("age", 30u64);
        let entity3 = Entity::create(create3, owner2).with_hash(1, 2, B256::ZERO);
        let key3 = entity3.key;
        db.add_entity(entity3).await;

        [key1, key2, key3]
    }

    #[tokio::test]
    async fn test_query_simple_string_equality() {
        let db = EntityDb::new();
        let keys = populate_test_entities(&db).await;

        // Query for entities with type = "user"
        let result = db.query_entities("type = \"user\"").await.unwrap();
        assert_eq!(result.len(), 2);

        // Should return user_high_priority (key1) and user_low_priority (key3)
        let result_keys: Vec<B256> = result.iter().map(|e| e.key).collect();
        assert!(result_keys.contains(&keys[0])); // user_high_priority
        assert!(result_keys.contains(&keys[2])); // user_low_priority
    }

    #[tokio::test]
    async fn test_query_simple_numeric_equality() {
        let db = EntityDb::new();
        let keys = populate_test_entities(&db).await;

        // Query for entities with priority = 5
        let result = db.query_entities("priority = 5").await.unwrap();
        assert_eq!(result.len(), 1);

        // Should return user_high_priority (key1) which has priority = 5
        assert_eq!(result[0].key, keys[0]);
    }

    #[tokio::test]
    async fn test_query_owner_equality() {
        let db = EntityDb::new();
        let keys = populate_test_entities(&db).await;

        let owner1 = "0x1234567890123456789012345678901234567890"
            .parse::<Address>()
            .unwrap();

        // Query for entities owned by owner1
        let result = db
            .query_entities(&format!("$owner = \"0x{:x}\"", owner1))
            .await
            .unwrap();
        assert_eq!(result.len(), 2);

        // Should return user_high_priority (key1) and admin (key2) which are owned by owner1
        let result_keys: Vec<B256> = result.iter().map(|e| e.key).collect();
        assert!(result_keys.contains(&keys[0])); // user_high_priority
        assert!(result_keys.contains(&keys[1])); // admin
    }

    #[tokio::test]
    async fn test_query_and_expression() {
        let db = EntityDb::new();
        let keys = populate_test_entities(&db).await;

        // Query for active users
        let result = db
            .query_entities("type = \"user\" && status = \"active\"")
            .await
            .unwrap();
        assert_eq!(result.len(), 1);

        // Should return user_high_priority (key1) which is both user and active
        assert_eq!(result[0].key, keys[0]);
    }

    #[tokio::test]
    async fn test_query_or_expression() {
        let db = EntityDb::new();
        let keys = populate_test_entities(&db).await;

        // Query for entities with priority 5 OR level 10
        let result = db
            .query_entities("priority = 5 || level = 10")
            .await
            .unwrap();
        assert_eq!(result.len(), 2);

        // Should return user_high_priority (key1) with priority 5 and admin (key2) with level 10
        let result_keys: Vec<B256> = result.iter().map(|e| e.key).collect();
        assert!(result_keys.contains(&keys[0])); // user_high_priority (priority 5)
        assert!(result_keys.contains(&keys[1])); // admin (level 10)
    }

    #[tokio::test]
    async fn test_query_parentheses() {
        let db = EntityDb::new();
        let keys = populate_test_entities(&db).await;

        // Query for: (type = "user" OR type = "admin") AND status = "active"
        let result = db
            .query_entities("(type = \"user\" || type = \"admin\") && status = \"active\"")
            .await
            .unwrap();
        assert_eq!(result.len(), 2);

        // Should return user_high_priority (key1) and admin (key2) which are both active
        let result_keys: Vec<B256> = result.iter().map(|e| e.key).collect();
        assert!(result_keys.contains(&keys[0])); // user_high_priority (user, active)
        assert!(result_keys.contains(&keys[1])); // admin (admin, active)
    }

    #[tokio::test]
    async fn test_query_complex_expression() {
        let db = EntityDb::new();
        let keys = populate_test_entities(&db).await;

        // Query for: (type = "user" AND priority = 5) OR (type = "admin" AND level = 10)
        let result = db
            .query_entities("(type = \"user\" && priority = 5) || (type = \"admin\" && level = 10)")
            .await
            .unwrap();
        assert_eq!(result.len(), 2);

        // Should return user_high_priority (key1) and admin (key2)
        let result_keys: Vec<B256> = result.iter().map(|e| e.key).collect();
        assert!(result_keys.contains(&keys[0])); // user_high_priority (user, priority 5)
        assert!(result_keys.contains(&keys[1])); // admin (admin, level 10)
    }

    #[tokio::test]
    async fn test_query_no_matches() {
        let db = EntityDb::new();
        let _keys = populate_test_entities(&db).await;

        // Query for non-existent annotation
        let result = db.query_entities("nonexistent = \"value\"").await.unwrap();
        assert_eq!(result.len(), 0);
    }

    #[tokio::test]
    async fn test_query_invalid_syntax() {
        let db = EntityDb::new();
        let _keys = populate_test_entities(&db).await;

        // Query with invalid syntax
        let result = db.query_entities("invalid syntax").await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Failed to parse query"));
    }

    #[tokio::test]
    async fn test_query_entity_keys_only() {
        let db = EntityDb::new();
        let keys = populate_test_entities(&db).await;

        // Query for entity keys only
        let result_keys = db.query_entity_keys("type = \"user\"").await.unwrap();
        assert_eq!(result_keys.len(), 2);

        // Should return user_high_priority (key1) and user_low_priority (key3)
        assert!(result_keys.contains(&keys[0])); // user_high_priority
        assert!(result_keys.contains(&keys[2])); // user_low_priority
    }

    #[tokio::test]
    async fn test_get_entities_by_owner() {
        let db = EntityDb::new();
        let keys = populate_test_entities(&db).await;

        let owner1 = "0x1234567890123456789012345678901234567890"
            .parse::<Address>()
            .unwrap();
        let owner2 = "0x0987654321098765432109876543210987654321"
            .parse::<Address>()
            .unwrap();

        // Get entities owned by owner1
        let owner1_keys = db.get_entities_by_owner(&owner1).await;
        assert_eq!(owner1_keys.len(), 2);
        assert!(owner1_keys.contains(&keys[0])); // user_high_priority
        assert!(owner1_keys.contains(&keys[1])); // admin

        // Get entities owned by owner2
        let owner2_keys = db.get_entities_by_owner(&owner2).await;
        assert_eq!(owner2_keys.len(), 1);
        assert_eq!(owner2_keys[0], keys[2]); // user_low_priority

        // Get entities for non-existent owner
        let non_existent_owner = "0x1111111111111111111111111111111111111111"
            .parse::<Address>()
            .unwrap();
        let non_existent_keys = db.get_entities_by_owner(&non_existent_owner).await;
        assert_eq!(non_existent_keys.len(), 0);
    }
}
