use anyhow::Result;
use serial_test::serial;

use arkiv_sdk::{
    client::ArkivClient,
    entity::{Create, Hash},
    rpc::QueryOptions,
    utils::{
        assert_numeric_annotation, assert_string_annotation, user_numeric_annotations,
        user_string_annotations,
    },
};
use arkiv_test_utils::{
    arkiv::{ArkivContainer, Config},
    create_test_account,
    entity_set::{
        create_expiration_test_entities, create_owner_test_entities, create_standard_test_entities,
    },
    init_logger,
};

#[tokio::test]
#[serial]
async fn test_get_entity_count() -> Result<()> {
    init_logger(false);

    let container = ArkivContainer::new(Config::default()).await?;
    let client = ArkivClient::new(container.get_url()?)?;
    let account = create_test_account(&client).await?;

    // Test initial count (should be 0)
    let initial_count = client.get_entity_count().await?;
    assert_eq!(initial_count, 0);

    // Create test entities
    let entity_ids = create_standard_test_entities(&client, account).await?;

    // Test count after creation
    let count_after_creation = client.get_entity_count().await?;
    assert_eq!(count_after_creation, entity_ids.len() as u64);

    // Remove one entity and test count
    client.remove_entries(account, vec![entity_ids[0]]).await?;
    let count_after_removal = client.get_entity_count().await?;
    assert_eq!(count_after_removal, (entity_ids.len() - 1) as u64);

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_get_all_entity_keys() -> Result<()> {
    init_logger(false);

    let container = ArkivContainer::new(Config::default()).await?;
    let client = ArkivClient::new(container.get_url()?)?;
    let account = create_test_account(&client).await?;

    // Test initial state (should be empty)
    let initial_keys = client.get_all_entity_keys().await?;
    assert!(initial_keys.is_empty());

    // Create test entities
    let entity_ids = create_standard_test_entities(&client, account).await?;

    // Test getting all entity keys
    let all_keys = client.get_all_entity_keys().await?;
    assert_eq!(all_keys.len(), entity_ids.len());

    // Verify all created entities are in the result
    for entity_id in &entity_ids {
        assert!(all_keys.contains(entity_id));
    }

    // Remove one entity and test again
    client.remove_entries(account, vec![entity_ids[0]]).await?;
    let keys_after_removal = client.get_all_entity_keys().await?;
    assert_eq!(keys_after_removal.len(), entity_ids.len() - 1);
    assert!(!keys_after_removal.contains(&entity_ids[0]));

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_get_entities_of_owner() -> Result<()> {
    init_logger(false);

    let container = ArkivContainer::new(Config::default()).await?;
    let client = ArkivClient::new(container.get_url()?)?;
    let account1 = create_test_account(&client).await?;
    let account2 = create_test_account(&client).await?;

    // Create entities for testing owner queries
    let (entity1, entity2, entity3) =
        create_owner_test_entities(&client, account1, account2).await?;

    // Test getting entities for account1
    let account1_entities = client.get_entities_of_owner(account1).await?;
    assert_eq!(account1_entities.len(), 2);
    assert!(account1_entities.contains(&entity1));
    assert!(account1_entities.contains(&entity2));

    // Test getting entities for account2
    let account2_entities = client.get_entities_of_owner(account2).await?;
    assert_eq!(account2_entities.len(), 1);
    assert!(account2_entities.contains(&entity3));

    // Test getting entities for non-existent account
    let non_existent_account = arkiv_sdk::Address::from([0u8; 20]);
    let non_existent_entities = client.get_entities_of_owner(non_existent_account).await?;
    assert!(non_existent_entities.is_empty());

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_get_storage_value() -> Result<()> {
    init_logger(false);

    let container = ArkivContainer::new(Config::default()).await?;
    let client = ArkivClient::new(container.get_url()?)?;
    let account = create_test_account(&client).await?;

    // Create test entity
    let test_payload = "test_storage_value_data";
    let entity_id = client
        .create_entry(
            account,
            Create::new(test_payload.as_bytes().to_vec(), 1000)
                .annotate_string("type", "storage_test"),
        )
        .await?;

    // Test getting storage value as string
    let storage_value: String = client.get_storage_value(entity_id).await?;
    assert_eq!(storage_value, test_payload);

    // Test getting storage value as bytes
    let storage_bytes: Vec<u8> = client.get_storage_value(entity_id).await?;
    assert_eq!(storage_bytes, test_payload.as_bytes());

    // Test getting storage value for non-existent entity
    let non_existent_id = Hash::from([0u8; 32]);
    let result: Result<String, _> = client.get_storage_value(non_existent_id).await;
    assert!(result.is_err());

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_query_with_options() -> Result<()> {
    init_logger(false);

    let container = ArkivContainer::new(Config::default()).await?;
    let client = ArkivClient::new(container.get_url()?)?;
    let account = create_test_account(&client).await?;

    // Create test entities
    let _entity_ids = create_standard_test_entities(&client, account).await?;

    // Test query with keys only
    let keys_only_options = QueryOptions::new().with_key();
    let keys_only_results = client
        .query_with_options("type = \"test\"", &keys_only_options)
        .await?;
    assert_eq!(keys_only_results.len(), 2);
    for result in &keys_only_results {
        assert!(result.value.is_none());
        assert!(result.string_annotations.is_empty());
        assert!(result.numeric_annotations.is_empty());
        assert!(result.expires_at.is_none());
        assert!(result.owner.is_none());
    }

    // Test query with all data
    let all_data_options = QueryOptions::with_all();
    let all_data_results = client
        .query_with_options("type = \"demo\"", &all_data_options)
        .await?;
    assert_eq!(all_data_results.len(), 2);
    for result in &all_data_results {
        assert!(result.value.is_some());
        assert!(!result.string_annotations.is_empty());
        assert!(!result.numeric_annotations.is_empty());
        assert!(result.expires_at.is_some());
        assert!(result.owner.is_some());
    }

    // Test query with metadata only (no payload)
    let metadata_only_options = QueryOptions::new()
        .with_key()
        .with_annotations(true)
        .with_expires_at()
        .with_owner_address()
        .exclude_payload();
    let metadata_results = client
        .query_with_options("category = \"alpha\"", &metadata_only_options)
        .await?;
    assert_eq!(metadata_results.len(), 2);
    for result in &metadata_results {
        assert!(result.value.is_none());
        assert!(!result.string_annotations.is_empty());
        assert!(!result.numeric_annotations.is_empty());
        assert!(result.expires_at.is_some());
        assert!(result.owner.is_some());
    }

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_query_entities() -> Result<()> {
    init_logger(false);

    let container = ArkivContainer::new(Config::default()).await?;
    let client = ArkivClient::new(container.get_url()?)?;
    let account = create_test_account(&client).await?;

    // Create test entities
    let entity_ids = create_standard_test_entities(&client, account).await?;

    // Test simple query
    let test_entities = client.query_entities("type = \"test\"").await?;
    assert_eq!(test_entities.len(), 2);
    for result in &test_entities {
        assert!(result.value.is_some());
        assert!(!result.string_annotations.is_empty());
        assert!(!result.numeric_annotations.is_empty());
        assert!(result.expires_at.is_some());
        assert!(result.owner.is_some());
    }

    // Test complex query with AND
    let and_results = client
        .query_entities("type = \"test\" && category = \"alpha\"")
        .await?;
    assert_eq!(and_results.len(), 1);
    assert_eq!(and_results[0].key, entity_ids[0]);

    // Test complex query with OR
    let or_results = client
        .query_entities("type = \"demo\" || category = \"beta\"")
        .await?;
    assert_eq!(or_results.len(), 3);
    assert!(or_results.iter().any(|r| r.key == entity_ids[1])); // entity2: category = "beta"
    assert!(or_results.iter().any(|r| r.key == entity_ids[2])); // entity3: type = "demo"
    assert!(or_results.iter().any(|r| r.key == entity_ids[3])); // entity4: type = "demo"

    // Test numeric annotation query
    let numeric_results = client.query_entities("priority = 1").await?;
    assert_eq!(numeric_results.len(), 2);
    assert!(numeric_results.iter().any(|r| r.key == entity_ids[0]));
    assert!(numeric_results.iter().any(|r| r.key == entity_ids[3]));

    // Test numeric inequality operators
    // Test != operator
    let not_equal_results = client.query_entities("priority != 1").await?;
    assert_eq!(not_equal_results.len(), 2);
    assert!(not_equal_results.iter().any(|r| r.key == entity_ids[1])); // priority = 2
    assert!(not_equal_results.iter().any(|r| r.key == entity_ids[2])); // priority = 3

    // Test < operator
    let less_than_results = client.query_entities("priority < 2").await?;
    assert_eq!(less_than_results.len(), 2);
    assert!(less_than_results.iter().any(|r| r.key == entity_ids[0])); // priority = 1
    assert!(less_than_results.iter().any(|r| r.key == entity_ids[3])); // priority = 1

    // Test > operator
    let greater_than_results = client.query_entities("priority > 2").await?;
    assert_eq!(greater_than_results.len(), 1);
    assert!(greater_than_results.iter().any(|r| r.key == entity_ids[2])); // priority = 3

    // Test >= operator
    let greater_equal_results = client.query_entities("priority >= 2").await?;
    assert_eq!(greater_equal_results.len(), 2);
    assert!(greater_equal_results.iter().any(|r| r.key == entity_ids[1])); // priority = 2
    assert!(greater_equal_results.iter().any(|r| r.key == entity_ids[2])); // priority = 3

    // Test <= operator
    let less_equal_results = client.query_entities("priority <= 2").await?;
    assert_eq!(less_equal_results.len(), 3);
    assert!(less_equal_results.iter().any(|r| r.key == entity_ids[0])); // priority = 1
    assert!(less_equal_results.iter().any(|r| r.key == entity_ids[1])); // priority = 2
    assert!(less_equal_results.iter().any(|r| r.key == entity_ids[3])); // priority = 1

    // Test mixed numeric queries combining different fields
    let mixed_and_results = client
        .query_entities("priority >= 2 && version <= 2")
        .await?;
    assert_eq!(mixed_and_results.len(), 2);
    assert!(mixed_and_results.iter().any(|r| r.key == entity_ids[1])); // priority = 2, version = 2
    assert!(mixed_and_results.iter().any(|r| r.key == entity_ids[2])); // priority = 3, version = 1

    let mixed_or_results = client
        .query_entities("priority != 2 || version >= 3")
        .await?;
    assert_eq!(mixed_or_results.len(), 3);
    assert!(mixed_or_results.iter().any(|r| r.key == entity_ids[0])); // priority = 1
    assert!(mixed_or_results.iter().any(|r| r.key == entity_ids[2])); // priority = 3
    assert!(mixed_or_results.iter().any(|r| r.key == entity_ids[3])); // version = 3

    // Test empty result
    let empty_results = client.query_entities("type = \"nonexistent\"").await?;
    assert!(empty_results.is_empty());

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_get_entity_metadata() -> Result<()> {
    init_logger(false);

    let container = ArkivContainer::new(Config::default()).await?;
    let client = ArkivClient::new(container.get_url()?)?;
    let account = create_test_account(&client).await?;

    // Create test entity
    let entity_id = client
        .create_entry(
            account,
            Create::new(b"metadata_test_data".to_vec(), 1000)
                .annotate_string("type", "metadata_test")
                .annotate_string("category", "test")
                .annotate_number("priority", 5)
                .annotate_number("version", 10),
        )
        .await?;

    // Test getting entity metadata
    let metadata = client.get_entity_metadata(entity_id).await?;

    // Verify all metadata fields
    assert_eq!(metadata.key, entity_id);
    assert!(metadata.value.is_some());
    assert_eq!(
        metadata.value.as_ref().unwrap().as_ref(),
        b"metadata_test_data"
    );
    assert!(metadata.expires_at.is_some());
    assert!(metadata.owner.is_some());
    assert_eq!(metadata.owner.unwrap(), account);

    // Verify string annotations
    assert_string_annotation(&metadata, "type", "metadata_test");
    assert_string_annotation(&metadata, "category", "test");

    // Verify numeric annotations
    assert_numeric_annotation(&metadata, "priority", 5);
    assert_numeric_annotation(&metadata, "version", 10);

    // Test getting metadata for non-existent entity
    let non_existent_id = Hash::from([0u8; 32]);
    let result: Result<_, _> = client.get_entity_metadata(non_existent_id).await;
    assert!(result.is_err());

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_get_entities_to_expire_at_block() -> Result<()> {
    init_logger(false);

    let container = ArkivContainer::new(Config::default()).await?;
    let client = ArkivClient::new(container.get_url()?)?;
    let account = create_test_account(&client).await?;

    // Get current block number
    let current_block = client.get_current_block_number().await?;

    // Create entities with different expiration blocks
    let (entity1, entity2, entity3) = create_expiration_test_entities(&client, account).await?;

    // Get actual expiration blocks from entity metadata
    // In dev mode, each entity creation produces a new block, so we need to get the actual expiration
    let metadata1 = client.get_entity_metadata(entity1).await?;
    let metadata2 = client.get_entity_metadata(entity2).await?;
    let metadata3 = client.get_entity_metadata(entity3).await?;

    let expire_at_block1 = metadata1.expires_at.unwrap();
    let expire_at_block2 = metadata2.expires_at.unwrap();
    let expire_at_block3 = metadata3.expires_at.unwrap();

    let entities_expiring_1000 = client
        .get_entities_to_expire_at_block(expire_at_block1)
        .await?;
    assert_eq!(entities_expiring_1000.len(), 1);
    assert!(entities_expiring_1000.contains(&entity1));

    let entities_expiring_2000 = client
        .get_entities_to_expire_at_block(expire_at_block2)
        .await?;
    assert_eq!(entities_expiring_2000.len(), 1);
    assert!(entities_expiring_2000.contains(&entity2));

    let entities_expiring_3000 = client
        .get_entities_to_expire_at_block(expire_at_block3)
        .await?;
    assert_eq!(entities_expiring_3000.len(), 1);
    assert!(entities_expiring_3000.contains(&entity3));

    // Test querying for a block with no expiring entities
    let no_expiring_entities = client
        .get_entities_to_expire_at_block(current_block + 5000)
        .await?;
    assert!(no_expiring_entities.is_empty());

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_query_entities_with_empty_annotations() -> Result<()> {
    init_logger(false);

    let container = ArkivContainer::new(Config::default()).await?;
    let client = ArkivClient::new(container.get_url()?)?;
    let account = create_test_account(&client).await?;

    // Create entities with no annotations
    let create_no_annotations = Create::new(b"no_annotations_data".to_vec(), 1000);
    let entity_without_annotations = client.create_entry(account, create_no_annotations).await?;

    // Create entity with only string annotations
    let create_string_only =
        Create::new(b"string_only_data".to_vec(), 1000).annotate_string("type", "string_only");
    let entity_with_string_only = client.create_entry(account, create_string_only).await?;

    // Create entity with only numeric annotations
    let create_numeric_only =
        Create::new(b"numeric_only_data".to_vec(), 1000).annotate_number("count", 5);
    let entity_with_numeric_only = client.create_entry(account, create_numeric_only).await?;

    // Create entity with both types of annotations
    let create_both = Create::new(b"both_annotations_data".to_vec(), 1000)
        .annotate_string("type", "both")
        .annotate_number("value", 10);
    let entity_with_both = client.create_entry(account, create_both).await?;

    // Test getting entity with no annotations by key
    let entity_no_annotations = client
        .get_entity_metadata(entity_without_annotations)
        .await?;

    // Verify that user annotations are empty (meta annotations may be present)
    let user_strings = user_string_annotations(&entity_no_annotations);
    let user_numerics = user_numeric_annotations(&entity_no_annotations);

    assert!(
        user_strings.is_empty(),
        "User string annotations should be empty, but found: {user_strings:?}"
    );
    assert!(
        user_numerics.is_empty(),
        "User numeric annotations should be empty, but found: {user_numerics:?}"
    );

    // Verify other fields are present
    assert!(
        entity_no_annotations.value.is_some(),
        "Entity value should be present"
    );
    assert_eq!(
        entity_no_annotations.value.as_ref().unwrap().as_ref(),
        b"no_annotations_data"
    );
    assert!(
        entity_no_annotations.expires_at.is_some(),
        "Expiration should be present"
    );
    assert!(
        entity_no_annotations.owner.is_some(),
        "Owner should be present"
    );
    assert_eq!(entity_no_annotations.owner.unwrap(), account);

    // Test getting entities with specific annotations to ensure they don't interfere
    let entity_string_only = client.get_entity_metadata(entity_with_string_only).await?;
    let user_strings = user_string_annotations(&entity_string_only);
    let user_numerics = user_numeric_annotations(&entity_string_only);
    assert!(!user_strings.is_empty());
    assert!(user_numerics.is_empty());

    let entity_numeric_only = client.get_entity_metadata(entity_with_numeric_only).await?;
    let user_strings = user_string_annotations(&entity_numeric_only);
    let user_numerics = user_numeric_annotations(&entity_numeric_only);
    assert!(user_strings.is_empty());
    assert!(!user_numerics.is_empty());

    let entity_both = client.get_entity_metadata(entity_with_both).await?;
    let user_strings = user_string_annotations(&entity_both);
    let user_numerics = user_numeric_annotations(&entity_both);
    assert!(!user_strings.is_empty());
    assert!(!user_numerics.is_empty());

    Ok(())
}
