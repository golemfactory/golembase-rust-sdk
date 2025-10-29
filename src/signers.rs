use alloy::primitives::{keccak256, Address};
use alloy::signers::k256::ecdsa::{SigningKey, VerifyingKey};
use alloy::signers::local::{LocalSignerError, PrivateKeySigner};
use alloy::signers::{Signature, SignerSync};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use rand::thread_rng;
use std::fs;
use std::path::PathBuf;

use crate::resilient_provider::ResilientProvider;
use crate::Hash;

/// Trait for signing transactions with different backends.
/// Implementors provide an address and a method to sign arbitrary data.
#[async_trait]
pub trait TransactionSigner: Send + Sync {
    /// Returns the address of the signer as an `Address`.
    fn address(&self) -> Address;

    /// Signs the given data and returns a `Signature`.
    async fn sign(&self, data: &[u8]) -> anyhow::Result<Signature>;
}

const DEFAULT_KEYSTORE_DIR: &str = "arkiv";

/// A signer that keeps the private key in memory and supports loading, saving, and listing keys.
/// Useful for local development and testing, with support for keystore files and raw key files.
pub struct InMemorySigner {
    signer: PrivateKeySigner,
}

impl InMemorySigner {
    /// Gets the default keystore directory path according to the XDG spec.
    /// Creates the directory if it does not exist.
    pub fn get_keystore_dir() -> anyhow::Result<PathBuf> {
        let path = dirs::config_dir()
            .context("Could not find home directory")?
            .join(DEFAULT_KEYSTORE_DIR);

        // Create directory only if it doesn't exist
        if !path.exists() {
            fs::create_dir_all(&path)?;
        }
        Ok(path)
    }

    /// Generates a new random private key and returns an in-memory signer.
    /// This is useful for creating new accounts programmatically.
    pub fn generate() -> Self {
        let signer = PrivateKeySigner::random();
        Self { signer }
    }

    /// Returns the private key as a `SigningKey`.
    /// Can be used for exporting or further cryptographic operations.
    pub fn private_key(&self) -> SigningKey {
        self.signer.credential().clone()
    }

    /// Returns the public key as a `VerifyingKey`.
    /// Useful for verifying signatures or exporting the public key.
    pub fn public_key(&self) -> VerifyingKey {
        *self.signer.credential().verifying_key()
    }

    /// Saves the private key to a file in the standard keystore directory using the provided password.
    /// The file is encrypted and named after the account address.
    pub fn save(&self, password: &str) -> anyhow::Result<PathBuf> {
        let path = Self::get_keystore_dir()?;
        let name = format!("key_{}.json", self.address());

        let mut rng = thread_rng();
        PrivateKeySigner::encrypt_keystore(
            &path,
            &mut rng,
            self.signer.credential().to_bytes(),
            password,
            Some(&name),
        )?;

        Ok(path)
    }

    /// Loads a private key from a keystore file at the given path using the provided password.
    /// Returns an in-memory signer if successful.
    pub fn load_keystore(path: PathBuf, password: &str) -> anyhow::Result<Self> {
        let signer = PrivateKeySigner::decrypt_keystore(&path, password).map_err(|e| match e {
            LocalSignerError::EcdsaError(e) => anyhow!("ECDSA error: {e}"),
            LocalSignerError::EthKeystoreError(e) => anyhow!("Keystore error: {e}"),
            e => anyhow!("Error loading key: {e}"),
        })?;
        Ok(Self { signer })
    }

    /// Loads a signer by address from the default keystore directory.
    /// Looks for a file named after the address and decrypts it with the given password.
    pub fn load_by_address(address: Address, password: &str) -> anyhow::Result<Self> {
        let path = Self::get_keystore_dir()?.join(format!("key_{}.json", address));
        Self::load_keystore(path, password)
    }

    /// Loads a signer from a raw private key file (not encrypted).
    /// Expects the file to contain the raw private key bytes.
    pub fn load_raw_key(path: PathBuf) -> anyhow::Result<Self> {
        let private_key_bytes =
            fs::read(&path).map_err(|e| anyhow!("Failed to read private key file: {}", e))?;

        let private_key = Hash::from_slice(&private_key_bytes);
        let signer = PrivateKeySigner::from_bytes(&private_key)
            .map_err(|e| anyhow!("Failed to parse private key: {}", e))?;

        Ok(Self { signer })
    }

    /// Lists all local accounts found in the keystore directory.
    /// Returns a vector of `Address` for each account found.
    pub fn list_local_accounts() -> anyhow::Result<Vec<Address>> {
        let keystore_dir = Self::get_keystore_dir()?;
        let mut accounts = Vec::new();

        if let Ok(entries) = std::fs::read_dir(keystore_dir) {
            for entry in entries.flatten() {
                if let Some(file_name) = entry.file_name().to_str() {
                    if let Some(address) = Self::parse_keystore_filename(file_name) {
                        accounts.push(address);
                    }
                }
            }
        }

        Ok(accounts)
    }

    /// Parses an `Address` from a keystore filename.
    /// Expects filenames in the format `key_{address}.json`.
    fn parse_keystore_filename(file_name: &str) -> Option<Address> {
        if !file_name.starts_with("key_") || !file_name.ends_with(".json") {
            return None;
        }

        file_name
            .strip_prefix("key_")
            .and_then(|s| s.strip_suffix(".json"))
            .and_then(|address_str| Address::parse_checksummed(address_str, None).ok())
    }
}

#[async_trait]
impl TransactionSigner for InMemorySigner {
    fn address(&self) -> Address {
        self.signer.address()
    }

    async fn sign(&self, data: &[u8]) -> anyhow::Result<Signature> {
        let hash = keccak256(data);
        Ok(self.signer.sign_hash_sync(&hash)?)
    }
}

/// A signer that uses Arkiv as a remote signing backend.
/// Intended for scenarios where signing is delegated to a node or service.
#[allow(dead_code)]
pub struct ArkivSigner {
    /// The address of the account as an `Address`.
    address: Address,
    /// The provider for signing, typically a remote node.
    provider: ResilientProvider,
    /// The chain ID for signing transactions.
    chain_id: u64,
}

impl ArkivSigner {
    /// Creates a new `ArkivSigner` with the given address, provider, and chain ID.
    pub fn new(address: Address, provider: ResilientProvider, chain_id: u64) -> Self {
        Self {
            address,
            provider,
            chain_id,
        }
    }
}

#[async_trait]
impl TransactionSigner for ArkivSigner {
    fn address(&self) -> Address {
        self.address
    }

    async fn sign(&self, _data: &[u8]) -> anyhow::Result<Signature> {
        unimplemented!()
    }
}
