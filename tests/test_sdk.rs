use alloy::primitives::B256;
use anyhow::{anyhow, Result};
use dirs::config_dir;
use serial_test::serial;
use std::fs;

use arkiv_sdk::{client::ArkivClient, entity::Create, PrivateKeySigner};
use arkiv_test_utils::{
    arkiv::{ArkivContainer, Config},
    init_logger,
};

fn get_client(container: &ArkivContainer) -> Result<ArkivClient> {
    let mut private_key_path =
        config_dir().ok_or_else(|| anyhow!("Failed to get config directory"))?;
    private_key_path.push("arkiv/private.key");
    let private_key_bytes = fs::read(&private_key_path)?;
    let private_key = B256::from_slice(&private_key_bytes);

    let signer = PrivateKeySigner::from_bytes(&private_key)
        .map_err(|e| anyhow!("Failed to parse private key: {}", e))?;

    let url = container.get_url()?;

    let client = ArkivClient::builder().wallet(signer).rpc_url(url).build();
    Ok(client)
}

#[ignore]
#[tokio::test]
#[serial]
async fn test_concurrent_entity_creation_batch_main_sdk() -> Result<()> {
    init_logger(false);

    // Start Arkiv container
    let container = ArkivContainer::new(Config::default()).await?;
    let client = get_client(&container)?;

    // Number of entities to create per task
    const ENTITIES_PER_TASK: usize = 15;

    // Spawn two tasks that will create entities concurrently
    let task1 = tokio::spawn({
        let client = client.clone();
        async move {
            let mut creates = Vec::new();
            for i in 0..ENTITIES_PER_TASK {
                let payload = format!("task1_entity_{}", i).into_bytes();
                let entry = Create::new(payload, 300)
                    .annotate_string("task", "task1")
                    .annotate_number("index", i as u64);
                creates.push(entry);
            }
            let results = client.create_entities(creates).await?;
            Ok::<_, anyhow::Error>(results)
        }
    });

    let task2 = tokio::spawn({
        let client = client.clone();
        async move {
            let mut creates = Vec::new();
            for i in 0..ENTITIES_PER_TASK {
                let payload = format!("task2_entity_{}", i).into_bytes();
                let entry = Create::new(payload, 300)
                    .annotate_string("task", "task2")
                    .annotate_number("index", i as u64);
                creates.push(entry);
            }
            let results = client.create_entities(creates).await?;
            Ok::<_, anyhow::Error>(results)
        }
    });

    // Wait for both tasks to complete
    let (task1_results, task2_results) = tokio::join!(task1, task2);
    let task1_entities = task1_results??;
    let task2_entities = task2_results??;

    // Verify all entities were created successfully
    for (i, result) in task1_entities.iter().enumerate() {
        let entry_str = client.cat(result.entity_key).await?;
        assert_eq!(entry_str, format!("task1_entity_{}", i));

        let metadata = client.get_entity_metadata(result.entity_key).await?;
        assert_eq!(metadata.string_annotations[0].value, "task1");
        assert_eq!(metadata.numeric_annotations[0].value, i as u64);
    }

    for (i, result) in task2_entities.iter().enumerate() {
        let entry_str = client.cat(result.entity_key).await?;
        assert_eq!(entry_str, format!("task2_entity_{}", i));

        let metadata = client.get_entity_metadata(result.entity_key).await?;
        assert_eq!(metadata.string_annotations[0].value, "task2");
        assert_eq!(metadata.numeric_annotations[0].value, i as u64);
    }

    log::info!(
        "Successfully verified {} concurrent batch entity creations",
        ENTITIES_PER_TASK * 2
    );
    Ok(())
}
