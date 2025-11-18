use anyhow::Result;
use serial_test::serial;

use arkiv_sdk::{
    client::ArkivClient,
    entity::Create,
    utils::{assert_numeric_annotation, assert_string_annotation},
};
use arkiv_test_utils::{
    arkiv::{ArkivContainer, Config},
    create_test_account, init_logger,
};

#[tokio::test]
#[serial]
async fn test_concurrent_entity_creation_batch() -> Result<()> {
    init_logger(false);

    // Start Arkiv container
    let container = ArkivContainer::new(Config::default()).await?;
    let client = ArkivClient::new(container.get_url()?)?;
    let account = create_test_account(&client).await?;

    // Number of entities to create per task
    const ENTITIES_PER_TASK: usize = 15;

    // Spawn two tasks that will create entities concurrently using create_entry
    let task1 = tokio::spawn({
        let client = client.clone();
        let account = account;
        async move {
            let mut entity_ids = Vec::new();
            for i in 0..ENTITIES_PER_TASK {
                let entry = Create::text(format!("task1_entity_{}", i), 300)
                    .annotate_string("task", "task1")
                    .annotate_number("index", i as u64);
                let entity_id = client.create_entry(account, entry).await?;
                entity_ids.push(entity_id);
            }
            Ok::<_, anyhow::Error>(entity_ids)
        }
    });

    let task2 = tokio::spawn({
        let client = client.clone();
        let account = account;
        async move {
            let mut entity_ids = Vec::new();
            for i in 0..ENTITIES_PER_TASK {
                let entry = Create::text(format!("task2_entity_{}", i), 300)
                    .annotate_string("task", "task2")
                    .annotate_number("index", i as u64);
                let entity_id = client.create_entry(account, entry).await?;
                entity_ids.push(entity_id);
            }
            Ok::<_, anyhow::Error>(entity_ids)
        }
    });

    // Wait for both tasks to complete
    let (task1_results, task2_results) = tokio::join!(task1, task2);
    let task1_entity_ids = task1_results??;
    let task2_entity_ids = task2_results??;

    // Verify all entities were created successfully
    for (i, entity_id) in task1_entity_ids.iter().enumerate() {
        let entry_str = client.cat(*entity_id).await?;
        assert_eq!(entry_str, format!("task1_entity_{}", i));

        let metadata = client.get_entity_metadata(*entity_id).await?;
        log::info!("Metadata: {metadata:?}");
        assert_string_annotation(&metadata, "task", "task1").unwrap();
        assert_numeric_annotation(&metadata, "index", i as u64).unwrap();
    }

    for (i, entity_id) in task2_entity_ids.iter().enumerate() {
        let entry_str = client.cat(*entity_id).await?;
        assert_eq!(entry_str, format!("task2_entity_{}", i));

        let metadata = client.get_entity_metadata(*entity_id).await?;
        assert_string_annotation(&metadata, "task", "task2").unwrap();
        assert_numeric_annotation(&metadata, "index", i as u64).unwrap();
    }

    log::info!(
        "Successfully verified {} concurrent single entity creations",
        ENTITIES_PER_TASK * 2
    );
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_batch_entity_creation() -> Result<()> {
    init_logger(false);

    // Start Arkiv container
    let container = ArkivContainer::new(Config::default()).await?;
    let client = ArkivClient::new(container.get_url()?)?;
    let account = create_test_account(&client).await?;

    // Create multiple entities in a single batch
    let mut creates = Vec::new();
    for i in 0..5 {
        let entry = Create::text(format!("batch_entity_{}", i), 300)
            .annotate_string("batch", "test")
            .annotate_number("index", i as u64);
        creates.push(entry);
    }

    // Use the batch creation function that returns entity IDs
    let entity_ids = client.create_entries(account, creates).await.unwrap();

    // Verify we got the expected number of entity IDs
    assert_eq!(entity_ids.len(), 5);

    // Verify each entity ID and retrieve the data.
    // This checks if entity IDs were retrieved in order.
    for (i, entity_id) in entity_ids.iter().enumerate() {
        // Verify we can retrieve the entity data
        let entry_str = client.cat(*entity_id).await?;
        assert_eq!(entry_str, format!("batch_entity_{}", i));

        // Verify metadata
        let metadata = client.get_entity_metadata(*entity_id).await?;
        assert_string_annotation(&metadata, "batch", "test").unwrap();
        assert_numeric_annotation(&metadata, "index", i as u64).unwrap();
        assert_eq!(metadata.owner.unwrap(), account);
    }

    log::info!(
        "Successfully verified batch entity creation with {} entity IDs",
        entity_ids.len()
    );
    Ok(())
}
