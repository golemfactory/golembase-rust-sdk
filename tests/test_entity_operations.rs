use anyhow::Result;
use golem_base_test_utils::find_entry_creation_transaction;
use serial_test::serial;
use std::time::{SystemTime, UNIX_EPOCH};

use golem_base_sdk::{
    client::GolemBaseClient,
    entity::{Create, Update},
};
use golem_base_test_utils::{
    create_test_account,
    golembase::{Config, GolemBaseContainer},
    init_logger,
};

#[tokio::test]
#[serial]
async fn test_create_and_retrieve_entry() -> Result<()> {
    init_logger(false);

    // Start GolemBase container
    let container = GolemBaseContainer::new(Config::default()).await?;
    let client = GolemBaseClient::new(container.get_url()?)?;
    let account = create_test_account(&client).await?;

    let test_payload = b"test payload".to_vec();
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

    let entry = Create::new(test_payload.clone(), 1000)
        .annotate_string("test_type", "Test")
        .annotate_number("test_timestamp", timestamp);

    let entry_id = client.create_entry(account, entry).await?;
    log::info!("Entry created with ID: 0x{entry_id:x}");

    let (tx, start_block) = find_entry_creation_transaction(&client, entry_id)
        .await?
        .unwrap();
    log::info!("Entry creation transaction: 0x{tx:x}");

    let entry_str = client.cat(entry_id).await?;
    log::info!("Retrieved entry 0x{entry_id:x}: {entry_str}");
    assert_eq!(entry_str, String::from_utf8(test_payload)?);

    let metadata = client.get_entity_metadata(entry_id).await?;
    log::info!("Retrieved metadata for entry 0x{entry_id:x}: {metadata:?}");

    assert_eq!(metadata.string_annotations[0].value, "Test");
    assert_eq!(metadata.numeric_annotations[0].value, timestamp);
    assert_eq!(metadata.owner.unwrap(), account);
    // Entry should be created in start_block + 1.
    assert_eq!(metadata.expires_at.unwrap(), start_block + 1000);
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_entity_operations() -> Result<()> {
    init_logger(false);

    // Start GolemBase container
    let container = GolemBaseContainer::new(Config::default()).await?;
    let client = GolemBaseClient::new(container.get_url()?)?;
    let account = create_test_account(&client).await?;

    // Create first entity
    let payload1 = b"first entity".to_vec();
    let timestamp1 = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let entry1 = Create::new(payload1.clone(), 1000)
        .annotate_string("test_type", "First")
        .annotate_number("test_timestamp", timestamp1);

    let entry1_id = client.create_entry(account, entry1).await?;
    log::info!("First entry created with ID: 0x{entry1_id:x}");

    // Create second entity
    let payload2 = b"second entity".to_vec();
    let timestamp2 = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let entry2 = Create::new(payload2.clone(), 1000)
        .annotate_string("test_type", "Second")
        .annotate_number("test_timestamp", timestamp2);

    let entry2_id = client.create_entry(account, entry2).await?;
    log::info!("Second entry created with ID: 0x{entry2_id:x}");

    // Verify both entities exist
    let entry1_str = client.cat(entry1_id).await?;
    let entry2_str = client.cat(entry2_id).await?;
    log::info!("Retrieved first entry 0x{entry1_id:x}: {entry1_str}");
    log::info!("Retrieved second entry 0x{entry2_id:x}: {entry2_str}");
    assert_eq!(entry1_str, String::from_utf8(payload1)?);
    assert_eq!(entry2_str, String::from_utf8(payload2)?);

    // Update first entity
    let updated_payload = b"updated first entity".to_vec();
    let updated_timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let update = Update::new(entry1_id, updated_payload.clone(), 1000)
        .annotate_string("test_type", "Updated")
        .annotate_number("test_timestamp", updated_timestamp);

    client.update_entry(account, update).await?;
    log::info!("First entry 0x{entry1_id:x} updated");

    // Verify first entity was updated
    let updated_str = client.cat(entry1_id).await?;
    log::info!("Retrieved updated first entry 0x{entry1_id:x}: {updated_str}");
    assert_eq!(updated_str, String::from_utf8(updated_payload.clone())?);

    // Remove second entity
    client.remove_entries(account, vec![entry2_id]).await?;
    log::info!("Second entry 0x{entry2_id:x} removed");

    // Verify second entity was removed
    let result = client.get_entity_metadata(entry2_id).await;
    assert!(
        result.is_err(),
        "Second entity 0x{entry2_id:x} should be removed. Instead got metadata: {:?}",
        result.unwrap()
    );
    let error = result.unwrap_err();
    assert!(
        error.to_string().contains("No entity found"),
        "Incorrect error message: {error}"
    );

    // Verify first entity still exists
    let final_str = client.cat(entry1_id).await?;
    log::info!("Retrieved final first entry 0x{entry1_id:x}: {final_str}");
    assert_eq!(final_str, String::from_utf8(updated_payload)?);

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_concurrent_entity_creation_batch() -> Result<()> {
    init_logger(false);

    // Start GolemBase container
    let container = GolemBaseContainer::new(Config::default()).await?;
    let client = GolemBaseClient::new(container.get_url()?)?;
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
                let payload = format!("task1_entity_{}", i).into_bytes();
                let entry = Create::new(payload, 300)
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
                let payload = format!("task2_entity_{}", i).into_bytes();
                let entry = Create::new(payload, 300)
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
        assert_eq!(metadata.string_annotations[0].value, "task1");
        assert_eq!(metadata.numeric_annotations[0].value, i as u64);
    }

    for (i, entity_id) in task2_entity_ids.iter().enumerate() {
        let entry_str = client.cat(*entity_id).await?;
        assert_eq!(entry_str, format!("task2_entity_{}", i));

        let metadata = client.get_entity_metadata(*entity_id).await?;
        assert_eq!(metadata.string_annotations[0].value, "task2");
        assert_eq!(metadata.numeric_annotations[0].value, i as u64);
    }

    log::info!(
        "Successfully verified {} concurrent single entity creations",
        ENTITIES_PER_TASK * 2
    );
    Ok(())
}
