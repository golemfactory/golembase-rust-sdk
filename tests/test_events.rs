use anyhow::Result;
use arkiv_sdk::ArkivClient;
use futures::StreamExt;
use serial_test::serial;
use std::time::Duration;

use arkiv_sdk::entity::{Create, Update};
use arkiv_sdk::events::Event;
use arkiv_test_utils::{
    arkiv::{ArkivContainer, Config},
    cleanup_entities, create_test_account, init_logger, TEST_TTL,
};

#[tokio::test]
#[serial]
async fn test_event_listening() -> Result<()> {
    init_logger(false);

    // Start Arkiv container
    let container = ArkivContainer::new(Config::default()).await?;
    let client = ArkivClient::new(container.get_url()?)?;
    let account = create_test_account(&client).await?;
    cleanup_entities(&client, account).await.unwrap();

    // Start listening for events, before we create the entity to avoid missing the event.
    let events = client.events_client().await.unwrap();
    let mut event_stream = events.events_stream().await.unwrap();

    // Create a test entity
    let create = Create::text("test payload", TEST_TTL);
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
    let update = Update::text(entity_id, "test payload", TEST_TTL);
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
#[serial]
async fn test_event_listening_with_timeout() -> Result<()> {
    init_logger(false);

    // Start Arkiv container
    let container = ArkivContainer::new(Config::default()).await?;
    let client = ArkivClient::new(container.get_url()?)?;
    let account = create_test_account(&client).await.unwrap();
    cleanup_entities(&client, account).await.unwrap();

    // Start listening for events
    let events = client.events_client().await.unwrap();
    let mut event_stream = events.events_stream().await.unwrap();

    // Create a test entity
    let create = Create::text("test payload", TEST_TTL);
    let entity_id = client.create_entry(account, create).await.unwrap();

    // Wait for event with timeout
    let event = tokio::time::timeout(Duration::from_secs(5), event_stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    match event {
        Event::EntityCreated { entity_id: id, .. } => {
            assert_eq!(id, entity_id);
        }
        _ => panic!("Expected EntityCreated event"),
    }
    Ok(())
}
