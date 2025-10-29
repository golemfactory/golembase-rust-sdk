use alloy::primitives::B256;
use alloy_rlp::{Encodable, RlpDecodable, RlpEncodable};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::convert::From;

/// A generic key-value pair structure for entity annotations.
/// Used for both string and numeric metadata attached to entities.
#[derive(Debug, Clone, Serialize, Deserialize, RlpEncodable, RlpDecodable)]
pub struct Annotation<T> {
    /// The key of the annotation.
    pub key: Key,
    /// The value of the annotation.
    pub value: T,
}

impl<T> Annotation<T> {
    /// Creates a new key-value pair annotation.
    /// Accepts any types convertible to `Key` and the annotation value.
    pub fn new<K, V>(key: K, value: V) -> Self
    where
        K: Into<Key>,
        V: Into<T>,
    {
        Annotation {
            key: key.into(),
            value: value.into(),
        }
    }
}

/// Type alias for string annotations (key-value pairs with `String` values).
pub type StringAnnotation = Annotation<String>;

/// Type alias for numeric annotations (key-value pairs with `u64` values).
pub type NumericAnnotation = Annotation<u64>;

/// A type alias for the hash used to identify entities in Arkiv.
pub type Hash = B256;

/// Type alias for the key used in annotations.
pub type Key = String;

/// Type representing a create transaction in Arkiv.
/// Used to define new entities, including their data, BTL, and annotations.
#[derive(Debug, Clone, Default, RlpEncodable, RlpDecodable, Deserialize)]
#[rlp(trailing)]
pub struct Create {
    /// The block-to-live (BTL) for the entity.
    pub btl: u64,
    /// The data associated with the entity.
    pub data: Bytes,
    /// String annotations for the entity.
    pub string_annotations: Vec<StringAnnotation>,
    /// Numeric annotations for the entity.
    pub numeric_annotations: Vec<NumericAnnotation>,
}

/// Type representing an update transaction in Arkiv.
/// Used to update existing entities, including their data, BTL, and annotations.
#[derive(Debug, Clone, Default, RlpEncodable, RlpDecodable, Deserialize)]
#[rlp(trailing)]
pub struct Update {
    /// The key of the entity to update.
    pub entity_key: Hash,
    /// The updated block-to-live (BTL) for the entity.
    pub btl: u64,
    /// The updated data for the entity.
    pub data: Bytes,
    /// Updated string annotations for the entity.
    pub string_annotations: Vec<StringAnnotation>,
    /// Updated numeric annotations for the entity.
    pub numeric_annotations: Vec<NumericAnnotation>,
}

/// Type alias for a delete operation (just the entity key).
pub type ArkivDelete = Hash;

/// Type representing an extend transaction in Arkiv.
/// Used to extend the BTL of an entity by a number of blocks.
#[derive(Debug, Clone, Default, RlpEncodable, RlpDecodable, Deserialize)]
pub struct Extend {
    /// The key of the entity to extend.
    pub entity_key: Hash,
    /// The number of blocks to extend the BTL by.
    pub number_of_blocks: u64,
}

/// Type representing a transaction in Arkiv, including creates, updates, deletes, and extensions.
/// Used as the main payload for submitting entity changes to the chain.
#[derive(Debug, Clone, Default, RlpEncodable, RlpDecodable)]
pub struct ArkivTransaction {
    /// A list of entities to create.
    pub creates: Vec<Create>,
    /// A list of entities to update.
    pub updates: Vec<Update>,
    /// A list of entity keys to delete.
    pub deletes: Vec<ArkivDelete>,
    /// A list of entities to extend.
    pub extensions: Vec<Extend>,
}

/// Represents an entity with data, BTL, and annotations.
/// Used for reading entity state from the chain.
#[derive(Debug, Clone, Default, RlpEncodable, RlpDecodable, Serialize, Deserialize)]
pub struct Entity {
    /// The data associated with the entity.
    pub data: String,
    /// The block-to-live (BTL) for the entity.
    pub btl: u64,
    /// String annotations for the entity.
    pub string_annotations: Vec<StringAnnotation>,
    /// Numeric annotations for the entity.
    pub numeric_annotations: Vec<NumericAnnotation>,
}

/// Represents the result of creating or updating an entity.
/// Contains the entity key and its expiration block.
#[derive(Debug, Clone, Default, RlpEncodable, RlpDecodable, Serialize)]
pub struct EntityResult {
    /// The key of the entity.
    pub entity_key: Hash,
    /// The block number at which the entity expires.
    pub expiration_block: u64,
}

/// Represents the result of extending an entity's BTL.
/// Contains the entity key, old expiration block, and new expiration block.
#[derive(Debug)]
pub struct ExtendResult {
    /// The key of the entity.
    pub entity_key: Hash,
    /// The old expiration block of the entity.
    pub old_expiration_block: u64,
    /// The new expiration block of the entity.
    pub new_expiration_block: u64,
}

/// Represents the result of deleting an entity.
/// Contains the key of the deleted entity.
#[derive(Debug)]
pub struct DeleteResult {
    /// The key of the entity that was deleted.
    pub entity_key: Hash,
}

impl Create {
    /// Creates a new `Create` operation with empty annotations.
    /// Accepts a payload as bytes and a BTL value.
    pub fn new(payload: Vec<u8>, btl: u64) -> Self {
        Self {
            btl,
            data: Bytes::from(payload),
            string_annotations: Vec::new(),
            numeric_annotations: Vec::new(),
        }
    }

    /// Creates a new `Create` request from any type that can be converted to `String`.
    pub fn from_string(payload: impl Into<String>, btl: u64) -> Self {
        Self {
            btl,
            data: Bytes::from(payload.into().into_bytes()),
            string_annotations: Vec::new(),
            numeric_annotations: Vec::new(),
        }
    }

    /// Adds a string annotation to the entity.
    /// Returns the modified `Create` for chaining.
    pub fn annotate_string(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.string_annotations.push(Annotation {
            key: key.into(),
            value: value.into(),
        });
        self
    }

    /// Adds a numeric annotation to the entity.
    /// Returns the modified `Create` for chaining.
    pub fn annotate_number(mut self, key: impl Into<String>, value: u64) -> Self {
        self.numeric_annotations.push(Annotation {
            key: key.into(),
            value,
        });
        self
    }
}

impl Update {
    /// Creates a new `Update` operation with empty annotations.
    /// Accepts an entity key, payload as bytes, and a BTL value.
    pub fn new(entity_key: B256, payload: Vec<u8>, btl: u64) -> Self {
        Self {
            entity_key,
            btl,
            data: Bytes::from(payload),
            string_annotations: Vec::new(),
            numeric_annotations: Vec::new(),
        }
    }

    /// Creates a new `Update` request from any type that can be converted to `String`.
    pub fn from_string<T: Into<String>>(entity_key: B256, payload: T, btl: u64) -> Self {
        Self {
            entity_key,
            btl,
            data: Bytes::from(payload.into().into_bytes()),
            string_annotations: Vec::new(),
            numeric_annotations: Vec::new(),
        }
    }

    /// Adds a string annotation to the entity.
    /// Returns the modified `Update` for chaining.
    pub fn annotate_string(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.string_annotations.push(Annotation {
            key: key.into(),
            value: value.into(),
        });
        self
    }

    /// Adds a numeric annotation to the entity.
    /// Returns the modified `Update` for chaining.
    pub fn annotate_number(mut self, key: impl Into<String>, value: u64) -> Self {
        self.numeric_annotations.push(Annotation {
            key: key.into(),
            value,
        });
        self
    }
}

impl ArkivTransaction {
    /// Returns the RLP-encoded bytes of the transaction.
    /// Useful for submitting the transaction to the chain.
    pub fn encoded(&self) -> Vec<u8> {
        let mut encoded = Vec::new();
        self.encode(&mut encoded);
        encoded
    }
}

// Tests check serialization compatibility with go implementation.
#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::B256;
    use hex;

    #[test]
    fn test_empty_transaction() {
        let tx = ArkivTransaction::default();
        assert_eq!(hex::encode(tx.encoded()), "c4c0c0c0c0");
    }

    #[test]
    fn test_create_without_annotations() {
        let create = Create::new(b"test payload".to_vec(), 1000);

        let mut tx = ArkivTransaction::default();
        tx.creates.push(create);

        assert_eq!(
            hex::encode(tx.encoded()),
            "d7d3d28203e88c74657374207061796c6f6164c0c0c0c0c0"
        );
    }

    #[test]
    fn test_create_with_annotations() {
        let create = Create::new(b"test payload".to_vec(), 1000)
            .annotate_string("foo", "bar")
            .annotate_number("baz", 42);

        let mut tx = ArkivTransaction::default();
        tx.creates.push(create);

        assert_eq!(
            hex::encode(tx.encoded()),
            "e6e2e18203e88c74657374207061796c6f6164c9c883666f6f83626172c6c58362617a2ac0c0c0"
        );
    }

    #[test]
    fn test_update_with_annotations() {
        let update = Update::new(
            B256::from_slice(&[1; 32]),
            b"updated payload".to_vec(),
            2000,
        )
        .annotate_string("status", "active")
        .annotate_number("version", 2);

        let mut tx = ArkivTransaction::default();
        tx.updates.push(update);

        assert_eq!(
            hex::encode(tx.encoded()),
            "f856c0f851f84fa001010101010101010101010101010101010101010101010101010101010101018207d08f75706461746564207061796c6f6164cfce8673746174757386616374697665cac98776657273696f6e02c0c0"
        );
    }

    #[test]
    fn test_delete_operation() {
        let mut tx = ArkivTransaction::default();
        tx.deletes.push(B256::from_slice(&[2; 32]));

        assert_eq!(
            hex::encode(tx.encoded()),
            "e5c0c0e1a00202020202020202020202020202020202020202020202020202020202020202c0"
        );
    }

    #[test]
    fn test_extend_btl() {
        let mut tx = ArkivTransaction::default();
        tx.extensions.push(Extend {
            entity_key: B256::from_slice(&[3; 32]),
            number_of_blocks: 500,
        });

        assert_eq!(
            hex::encode(tx.encoded()),
            "e9c0c0c0e5e4a003030303030303030303030303030303030303030303030303030303030303038201f4"
        );
    }

    #[test]
    fn test_mixed_operations() {
        let create = Create::new(b"test payload".to_vec(), 1000).annotate_string("type", "test");
        let update = Update::new(
            B256::from_slice(&[1; 32]),
            b"updated payload".to_vec(),
            2000,
        );
        let mut tx = ArkivTransaction::default();
        tx.creates.push(create);
        tx.updates.push(update);
        tx.deletes.push(B256::from_slice(&[2; 32]));
        tx.extensions.push(Extend {
            entity_key: B256::from_slice(&[3; 32]),
            number_of_blocks: 500,
        });

        assert_eq!(
            hex::encode(tx.encoded()),
            "f89fdedd8203e88c74657374207061796c6f6164cbca84747970658474657374c0f7f6a001010101010101010101010101010101010101010101010101010101010101018207d08f75706461746564207061796c6f6164c0c0e1a00202020202020202020202020202020202020202020202020202020202020202e5e4a003030303030303030303030303030303030303030303030303030303030303038201f4"
        );
    }
}
