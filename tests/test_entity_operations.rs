use anyhow::Result;
use arkiv_test_utils::find_entry_creation_transaction;
use serial_test::serial;
use std::time::{SystemTime, UNIX_EPOCH};

use arkiv_sdk::{
    client::ArkivClient,
    entity::{Create, Update},
    utils::{assert_numeric_annotation, assert_string_annotation},
};
use arkiv_test_utils::{
    arkiv::{ArkivContainer, Config},
    create_test_account, init_logger,
};

#[tokio::test]
#[serial]
async fn test_create_and_retrieve_entry() -> Result<()> {
    init_logger(false);

    // Start Arkiv container
    let container = ArkivContainer::new(Config::default()).await?;
    let client = ArkivClient::new(container.get_url()?)?;
    let account = create_test_account(&client).await?;

    let test_payload = "test payload";
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

    let entry = Create::text(test_payload, 1000)
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
    assert_eq!(entry_str, test_payload);

    let metadata = client.get_entity_metadata(entry_id).await?;
    log::info!("Retrieved metadata for entry 0x{entry_id:x}: {metadata:?}");

    assert_string_annotation(&metadata, "test_type", "Test").unwrap();
    assert_numeric_annotation(&metadata, "test_timestamp", timestamp).unwrap();
    assert_eq!(metadata.owner.unwrap(), account);
    // Entry should be created in start_block + 1.
    assert_eq!(metadata.expires_at.unwrap(), start_block + 1000);
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_entity_operations() -> Result<()> {
    init_logger(false);

    // Start Arkiv container
    let container = ArkivContainer::new(Config::default()).await?;
    let client = ArkivClient::new(container.get_url()?)?;
    let account = create_test_account(&client).await?;

    // Create first entity
    let payload1 = "first entity";
    let timestamp1 = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let entry1 = Create::text(payload1, 1000)
        .annotate_string("test_type", "First")
        .annotate_number("test_timestamp", timestamp1);

    let entry1_id = client.create_entry(account, entry1).await?;
    log::info!("First entry created with ID: 0x{entry1_id:x}");

    // Create second entity
    let payload2 = "second entity";
    let timestamp2 = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let entry2 = Create::text(payload2, 1000)
        .annotate_string("test_type", "Second")
        .annotate_number("test_timestamp", timestamp2);

    let entry2_id = client.create_entry(account, entry2).await?;
    log::info!("Second entry created with ID: 0x{entry2_id:x}");

    // Verify both entities exist
    let entry1_str = client.cat(entry1_id).await?;
    let entry2_str = client.cat(entry2_id).await?;
    log::info!("Retrieved first entry 0x{entry1_id:x}: {entry1_str}");
    log::info!("Retrieved second entry 0x{entry2_id:x}: {entry2_str}");
    assert_eq!(entry1_str, payload1);
    assert_eq!(entry2_str, payload2);

    // Update first entity
    let updated_payload = "updated first entity";
    let updated_timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let update = Update::text(entry1_id, updated_payload, 1000)
        .annotate_string("test_type", "Updated")
        .annotate_number("test_timestamp", updated_timestamp);

    client.update_entry(account, update).await?;
    log::info!("First entry 0x{entry1_id:x} updated");

    // Verify first entity was updated
    let updated_str = client.cat(entry1_id).await?;
    log::info!("Retrieved updated first entry 0x{entry1_id:x}: {updated_str}");
    assert_eq!(updated_str, updated_payload);

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
    assert_eq!(final_str, updated_payload);

    Ok(())
}
