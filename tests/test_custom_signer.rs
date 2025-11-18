use alloy::primitives::{keccak256, Address};
use alloy::signers::local::PrivateKeySigner;
use alloy::signers::{Signature, SignerSync};
use anyhow::Result;
use async_trait::async_trait;
use bigdecimal::BigDecimal;
use serial_test::serial;
use std::sync::Arc;
use tokio::sync::Mutex;

use arkiv_sdk::{client::ArkivClient, entity::Create, signers::TransactionSigner};
use arkiv_test_utils::{
    arkiv::{ArkivContainer, Config},
    init_logger, TEST_TTL,
};

/// A custom signer that uses tokio::task::spawn_local for signing
struct LocalTaskSigner {
    inner: Arc<Mutex<PrivateKeySigner>>,
    address: Address,
}

impl LocalTaskSigner {
    fn new(signer: PrivateKeySigner) -> Self {
        let address = signer.address();
        Self {
            inner: Arc::new(Mutex::new(signer)),
            address,
        }
    }
}

#[async_trait]
impl TransactionSigner for LocalTaskSigner {
    fn address(&self) -> Address {
        self.address
    }

    async fn sign(&self, data: &[u8]) -> Result<Signature> {
        let hash = keccak256(data);
        let signer = self.inner.clone();

        // Use spawn_local to run the signing operation in a local task
        let signature = tokio::task::spawn_local(async move {
            let signer = signer.lock().await;
            signer.sign_hash_sync(&hash)
        })
        .await??;

        Ok(signature)
    }
}

/// Test the custom signer with tokio spawn_local used in signer function.
/// Using spawn_local can panic when is called outside of LocalSet.
#[tokio::test]
#[serial]
async fn test_custom_signer_with_spawn_local() -> Result<()> {
    init_logger(false);

    // Start Arkiv container
    let container = ArkivContainer::new(Config::default()).await?;
    let client = ArkivClient::new(container.get_url()?)?;

    // Create a custom signer
    let inner_signer = PrivateKeySigner::random();
    let custom_signer = LocalTaskSigner::new(inner_signer);

    // Register the custom signer
    let account = client.account_register(custom_signer).await?;
    client.fund(account, BigDecimal::from(1)).await?;
    log::info!("Registered custom signer account: {account}");

    // Create a test entity
    let entry = Create::from_string("test payload from custom signer", TEST_TTL)
        .annotate_string("test_type", "CustomSignerTest");

    let entry_id = client.create_entry(account, entry).await?;
    log::info!("Created entity with ID: 0x{entry_id:x}");

    // Verify the entity was created
    let entry_str = client.cat(entry_id).await?;
    log::info!("Retrieved entry 0x{entry_id:x}: {entry_str}");
    assert_eq!(entry_str, "test payload from custom signer");

    Ok(())
}
