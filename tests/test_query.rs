use anyhow::Result;
use serial_test::serial;

use arkiv_sdk::{client::ArkivClient, entity::Create};
use arkiv_test_utils::{
    arkiv::{ArkivContainer, Config},
    cleanup_entities, create_test_account, init_logger,
};

#[tokio::test]
#[serial]
async fn test_query_entities() -> Result<()> {
    init_logger(false);

    // Start Arkiv container
    let container = ArkivContainer::new(Config::default()).await?;
    let client = ArkivClient::new(container.get_url()?)?;

    // Create test account
    let account = create_test_account(&client).await?;
    cleanup_entities(&client, account).await?;

    // Create entries with different annotations
    let entry1 = Create::new(b"test1".to_vec(), 1000)
        .annotate_string("type", "test")
        .annotate_string("category", "alpha");
    let entry1_id = client.create_entry(account, entry1).await?;
    log::info!("Created entry1: {entry1_id}");

    let entry2 = Create::new(b"test2".to_vec(), 1000)
        .annotate_string("type", "test")
        .annotate_string("category", "beta");
    let entry2_id = client.create_entry(account, entry2).await?;
    log::info!("Created entry2: {entry2_id}");

    let entry3 = Create::new(b"test3".to_vec(), 1000)
        .annotate_string("type", "demo")
        .annotate_string("category", "alpha");
    let entry3_id = client.create_entry(account, entry3).await?;
    log::info!("Created entry3: {entry3_id}");

    // Test queries
    let type_test_entries = client.query_entity_keys("type = \"test\"").await?;
    log::info!("Entries with type = \"test\": {:?}", type_test_entries);
    assert_eq!(type_test_entries.len(), 2);
    assert!(type_test_entries.contains(&entry1_id));
    assert!(type_test_entries.contains(&entry2_id));

    let category_alpha_entries = client.query_entity_keys("category = \"alpha\"").await?;
    log::info!(
        "Entries with category = \"alpha\": {:?}",
        category_alpha_entries
    );
    assert_eq!(category_alpha_entries.len(), 2);
    assert!(category_alpha_entries.contains(&entry1_id));
    assert!(category_alpha_entries.contains(&entry3_id));

    let type_demo_entries = client.query_entity_keys("type = \"demo\"").await?;
    log::info!("Entries with type = \"demo\": {:?}", type_demo_entries);
    assert_eq!(type_demo_entries.len(), 1);
    assert!(type_demo_entries.contains(&entry3_id));

    let combined_and = client
        .query_entity_keys("type = \"test\" && category = \"beta\"")
        .await?;
    log::info!(
        "Entries with type = \"test\" && category = \"beta\": {:?}",
        combined_and
    );
    assert_eq!(combined_and.len(), 1);
    assert!(combined_and.contains(&entry2_id));

    let combined_or = client
        .query_entity_keys("type = \"demo\" || category = \"beta\"")
        .await?;
    log::info!(
        "Entries with type = \"demo\" || category = \"beta\": {:?}",
        combined_or
    );
    assert_eq!(combined_or.len(), 2);
    assert!(combined_or.contains(&entry2_id));
    assert!(combined_or.contains(&entry3_id));

    // Test empty result
    let no_results = client.query_entity_keys("type = \"nonexistent\"").await?;
    log::info!("Entries with type = \"nonexistent\": {:?}", no_results);
    assert_eq!(no_results.len(), 0);

    // Test selecting all entries
    let all_entries = client
        .query_entity_keys("type = \"test\" || type = \"demo\"")
        .await?;
    log::info!("All entries: {:?}", all_entries);
    assert_eq!(all_entries.len(), 3);
    assert!(all_entries.contains(&entry1_id));
    assert!(all_entries.contains(&entry2_id));
    assert!(all_entries.contains(&entry3_id));

    Ok(())
}
