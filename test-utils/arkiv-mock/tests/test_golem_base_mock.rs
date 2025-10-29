use std::time::Duration;

use bigdecimal::BigDecimal;
use futures::StreamExt;
use arkiv_mock::{
    controller::{CallOverride, CallResponse, CallbackResult},
    ArkivMockServer,
};
use arkiv_sdk::{
    entity::{Create, Update},
    events::Event,
    ArkivClient,
};
use arkiv_test_utils::{create_test_account, init_logger, TEST_TTL};
use serial_test::serial;

/// Comprehensive integration test that demonstrates using the Arkiv mock server with ArkivClient
#[tokio::test]
//#[serial]
async fn test_golem_base_mock_integration() -> anyhow::Result<()> {
    init_logger(false);

    // Test 1: Basic functionality with default mock server
    log::info!("Testing basic functionality with default mock server...");
    let server = ArkivMockServer::create_test_mock_server().await?;
    let client = ArkivClient::new(server.url().clone())?;

    // Are we able to get the chain id?
    let chain_id = client.get_chain_id().await?;
    assert_eq!(chain_id, 1337);

    // Create a test account with funding
    log::info!("Creating test account and funding it...");
    let account = create_test_account(&client).await.unwrap();
    let balance = client.get_balance(account).await?;
    assert!(balance == BigDecimal::from(1));

    log::info!("Account {account} created with balance {balance}");

    // Test basic entity creation
    log::info!("Creating test entity...");

    let test_data = b"Hello, Arkiv!";
    let create = Create::new(test_data.to_vec(), 100)
        .annotate_string("test_type", "Test")
        .annotate_number("test_timestamp", 1234567890);

    let result = client.create_entry(account, create).await.unwrap();

    log::info!("Created entity {result}...");

    log::info!("Retrieving storage value for entity key {result}...");
    let storage_value = client.get_storage_value::<Vec<u8>>(result).await.unwrap();
    let storage_string = String::from_utf8(storage_value.clone()).unwrap();
    log::info!("Storage value: {}", storage_string);
    assert_eq!(storage_value, test_data);

    log::info!("Querying entities by string annotation 'test_type = \"Test\"'...");
    let test_type_results = client.query_entities("test_type = \"Test\"").await.unwrap();
    log::info!(
        "Found {} entities with test_type = 'Test'",
        test_type_results.len()
    );
    assert_eq!(test_type_results.len(), 1);
    assert_eq!(test_type_results[0].key, result);
    assert_eq!(
        test_type_results[0].value.clone().unwrap(),
        test_data.as_slice()
    );

    log::info!("Querying entities by numeric annotation 'test_timestamp = 1234567890'...");
    let timestamp_results = client
        .query_entities("test_timestamp = 1234567890")
        .await
        .unwrap();
    log::info!(
        "Found {} entities with test_timestamp = 1234567890",
        timestamp_results.len()
    );
    assert_eq!(timestamp_results.len(), 1);
    assert_eq!(timestamp_results[0].key, result);

    log::info!("Querying entity keys by annotation...");
    let test_type_keys = client
        .query_entity_keys("test_type = \"Test\"")
        .await
        .unwrap();
    assert_eq!(test_type_keys.len(), 1);
    assert_eq!(test_type_keys[0], result);

    log::info!("Getting entity metadata...");
    let metadata = client.get_entity_metadata(result).await.unwrap();
    log::info!("Entity metadata: {:?}", metadata);
    assert_eq!(metadata.owner.unwrap(), account);
    assert_eq!(metadata.string_annotations.len(), 1);
    assert_eq!(metadata.numeric_annotations.len(), 1);

    log::info!("✅ All Arkiv mock tests completed successfully!");
    Ok(())
}

// Test triggering callback for CallOverride::Once.
#[tokio::test]
//#[serial]
async fn test_golem_base_mock_once_callback_waiting() -> anyhow::Result<()> {
    init_logger(false);

    let mock = ArkivMockServer::create_test_mock_server().await?;
    let ctrl = mock.controller();
    let client = ArkivClient::new(mock.url().clone())?;
    let account = create_test_account(&client).await.unwrap();

    log::info!("Create callbacks first to be sure that they will be triggered in correct order");
    let mut call1 = ctrl.override_rpc(
        "eth_getBalance",
        CallOverride::Once(CallResponse::Error("error sending request".to_string())),
    );
    let mut call2 = ctrl.override_rpc("eth_getBalance", CallOverride::Once(CallResponse::Success));

    log::info!("Create a task that will get balance, what will trigger callbacks internally");
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(1)).await;

        let balance = client.get_balance(account).await.unwrap();
        log::info!("Balance: {balance}");
    });

    log::info!("First callback should be triggered after async task will finish sleeping");
    call1.triggered(Duration::from_millis(1200)).await.unwrap();
    log::info!("✅ First callback triggered");

    log::info!("Second callback should be triggered immediately, because get_balance will retry");
    call2.triggered(Duration::from_millis(100)).await.unwrap();
    log::info!("✅ Second callback triggered");

    log::info!("Validating if channel won't return anything after override was used.");
    let result1 = call1
        .wait_for_trigger(Duration::from_millis(1))
        .await
        .unwrap();
    matches!(result1, CallbackResult::ChannelDropped);

    let result2 = call2
        .wait_for_trigger(Duration::from_millis(1))
        .await
        .unwrap();
    matches!(result2, CallbackResult::ChannelDropped);

    Ok(())
}

#[tokio::test]
//#[serial]
async fn test_golem_base_mock_event_listening() -> anyhow::Result<()> {
    init_logger(false);

    let mock = ArkivMockServer::create_test_mock_server().await?;
    let client = ArkivClient::new(mock.url().clone())?;
    let account = create_test_account(&client).await?;

    // Start listening for events, before we create the entity to avoid missing the event.
    let events = client.events_client().await.unwrap();
    let mut event_stream = events.events_stream().await.unwrap();

    // Create a test entity
    let create = Create::from_string("test payload", TEST_TTL);
    let entity_id = client.create_entry(account, create).await.unwrap();

    // Wait for and verify EntityCreated event
    let event = event_stream.next().await.unwrap().unwrap();
    log::info!("Event: {:?}", event);
    match event {
        Event::EntityCreated { entity_id: id, .. } => {
            assert_eq!(id, entity_id);
        }
        _ => panic!("Expected EntityCreated event"),
    }

    // Update the entity
    let update = Update::from_string(entity_id, "test payload", TEST_TTL);
    client.update_entry(account, update).await.unwrap();

    // Wait for and verify EntityUpdated event
    let event = event_stream.next().await.unwrap().unwrap();
    match event {
        Event::EntityUpdated { entity_id: id, .. } => {
            assert_eq!(id, entity_id);
        }
        _ => panic!("Expected EntityUpdated event"),
    }

    // Delete the entity
    client
        .remove_entries(account, vec![entity_id])
        .await
        .unwrap();

    // Wait for and verify EntityRemoved event
    let event = event_stream.next().await.unwrap().unwrap();
    match event {
        Event::EntityRemoved { entity_id: id, .. } => {
            assert_eq!(id, entity_id);
        }
        _ => panic!("Expected EntityRemoved event"),
    }
    Ok(())
}

#[tokio::test]
//#[serial]
async fn test_golem_base_mock_expiration() -> anyhow::Result<()> {
    init_logger(false);

    let mock = ArkivMockServer::create_test_mock_server().await?;
    let client = ArkivClient::new(mock.url().clone())?;
    let account = create_test_account(&client).await?;

    let events = client.events_client().await.unwrap();
    let mut event_stream = events.events_stream().await.unwrap();

    let entity = Create::new(b"test payload".to_vec(), 1);
    let entity_id = client.create_entry(account, entity).await?;

    // Ignore EntityCreated event.
    event_stream.next().await.unwrap().unwrap();
    let event = tokio::time::timeout(Duration::from_secs(5), event_stream.next())
        .await
        .expect("Expected Entity to be removed within 2 seconds due to expiration")
        .unwrap()
        .unwrap();
    match event {
        Event::EntityRemoved { entity_id: id, .. } => {
            assert_eq!(id, entity_id);
        }
        Event::EntityCreated { .. } => panic!("Expected EntityRemoved event, got EntityCreated"),
        Event::EntityUpdated { .. } => panic!("Expected EntityRemoved event, got EntityUpdated"),
    }
    Ok(())
}
