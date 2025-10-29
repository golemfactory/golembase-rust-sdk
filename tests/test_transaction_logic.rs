use std::time::Duration;

use alloy::primitives::U256;
use arkiv_mock::{
    controller::{CallOverride, CallResponse},
    ArkivMockServer,
};
use arkiv_sdk::{client::TransactionConfig, entity::Create, ArkivClient};
use arkiv_test_utils::{
    create_test_account,
    arkiv::{Config, ArkivContainer},
    init_logger,
};
use serial_test::serial;

const NUM_ITERATIONS: usize = 50;

#[tokio::test]
#[serial]
async fn test_transaction_random_errors() -> anyhow::Result<()> {
    init_logger(false);

    let mock = ArkivMockServer::create_test_mock_server().await?;
    let ctrl = mock.controller();
    let client = ArkivClient::new(mock.url().clone())?;
    let account = create_test_account(&client).await.unwrap();

    let _callback = ctrl.global_override(CallOverride::Always(CallResponse::FailEachNth {
        error: "error sending request".to_string(),
        frequency: 2,
    }));

    for i in 0..NUM_ITERATIONS {
        let create = Create::from_string("Hello, GolemBase!", 100)
            .annotate_string("test_type", "Test")
            .annotate_number("test_timestamp", 1234567890)
            .annotate_number("iteration", i as u64);

        let result = client.create_entry(account, create).await.unwrap();
        log::info!("Created entity {result} in iteration {i}...");
    }

    log::info!(
        "✅ Successfully created {} entities with deterministic error handling (every 3rd request fails)!",
        NUM_ITERATIONS
    );
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_transaction_indexing_in_progress() -> anyhow::Result<()> {
    init_logger(false);

    let mock = ArkivMockServer::create_test_mock_server().await?;
    let ctrl = mock.controller();
    let client = ArkivClient::new(mock.url().clone())?;
    let account = create_test_account(&client).await.unwrap();

    let _callback = ctrl.override_rpc(
        "eth_getTransactionReceipt",
        CallOverride::NTimes {
            response: CallResponse::Error("transaction indexing is in progress".to_string()),
            n: 4,
        },
    );

    let create = Create::from_string("Hello, GolemBase!", 100)
        .annotate_string("test_type", "Test")
        .annotate_number("test_timestamp", 1234567890);

    let result = client.create_entry(account, create).await.unwrap();
    log::info!("Created entity {result}...");

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_transaction_nonce_too_low() -> anyhow::Result<()> {
    init_logger(false);

    let mock = ArkivMockServer::create_test_mock_server().await?;
    let ctrl = mock.controller();
    let client = ArkivClient::new(mock.url().clone())?;
    let account = create_test_account(&client).await.unwrap();

    let create = Create::from_string("Hello, GolemBase!", 100);
    let result = client.create_entry(account, create).await.unwrap();
    log::info!("Created first entity {result}...");

    let nonce = client
        .get_rpc_client()
        .get_transaction_count(account)
        .await
        .unwrap();

    // Simulating situation when we have RPC switch and the new instance doesn't know about
    // the previous transaction yet.
    ctrl.override_rpc(
        "eth_getTransactionCount",
        CallOverride::Once(CallResponse::custom(&U256::from(nonce - 1)).unwrap()),
    );

    log::info!("Creating entity with nonce too low...");
    let create = Create::from_string("Hello 2", 100);
    let result = client.create_entry(account, create).await.unwrap();
    log::info!("Created entity {result}...");

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_transaction_nonce_too_low_new_client() -> anyhow::Result<()> {
    init_logger(false);

    let mock = ArkivMockServer::create_test_mock_server().await?;
    let ctrl = mock.controller();
    let client = ArkivClient::new(mock.url().clone())?;
    let account = create_test_account(&client).await.unwrap();

    let create = Create::from_string("Hello, GolemBase!", 100);
    let result = client.create_entry(account, create).await.unwrap();
    log::info!("Created first entity {result}...");

    let nonce = client
        .get_rpc_client()
        .get_transaction_count(account)
        .await
        .unwrap();

    // Simulating situation when we have RPC switch and the new instance doesn't know about
    // the previous transaction yet.
    ctrl.override_rpc(
        "eth_getTransactionCount",
        CallOverride::Once(CallResponse::custom(&U256::from(nonce - 1)).unwrap()),
    );

    // New client doesn't have previous nonce stored, so it will get the error.
    // This simulates scenario when application using the client is restarted.
    let client2 = ArkivClient::new(mock.url().clone())?;
    client2.account_load(account, "test123").await?;

    log::info!("Creating entity with nonce too low...");
    let create = Create::from_string("Hello 2", 100);
    let result = client2.create_entry(account, create).await.unwrap();
    log::info!("Created entity {result}...");

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_transaction_wait_for_confirmations() -> anyhow::Result<()> {
    init_logger(false);

    let mock = ArkivMockServer::create_test_mock_server().await?;
    let client = ArkivClient::new(mock.url().clone())?.override_config(TransactionConfig {
        required_confirmations: 1,
        ..TransactionConfig::default()
    });
    let account = create_test_account(&client).await.unwrap();

    let create = Create::from_string("Hello, GolemBase!", 100);
    let result = client.create_entry(account, create).await.unwrap();
    log::info!("Created first entity {result}...");
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_transaction_rpc_pause() -> anyhow::Result<()> {
    init_logger(false);

    let container = ArkivContainer::new(Config::default().with_port(33221)).await?;
    let client = ArkivClient::new(container.get_url()?)?;
    let account = create_test_account(&client).await.unwrap();

    let create = Create::from_string("Hello, GolemBase!", 100);
    let result = client.create_entry(account, create).await.unwrap();
    log::info!("Created first entity {result}...");

    // Restarting container to check if transaction logic will be able to handle the situation.
    container.pause().await.unwrap();

    log::info!("Creating entity when RPC is down... It should fail.");
    let result = client
        .create_entry(account, Create::from_string("Hello 2", 100))
        .await;
    assert!(result.is_err());
    container.unpause().await.unwrap();

    log::info!("Creating entity after RPC restart... It should succeed.");
    let create = Create::from_string("Hello 3", 100);
    let result = client.create_entry(account, create).await.unwrap();
    log::info!("Created entity {result}...");

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_transaction_rpc_restart() -> anyhow::Result<()> {
    init_logger(false);

    let mut container =
        ArkivContainer::new(Config::default().with_port(33221).preserve_volume()).await?;
    let client = ArkivClient::new(container.get_url()?)?;
    let account = create_test_account(&client).await.unwrap();

    let create = Create::from_string("Hello, GolemBase!", 100);
    let result = client.create_entry(account, create).await.unwrap();
    log::info!("Created first entity {result}...");

    // Restarting container to check if transaction logic will be able to handle the situation.
    log::info!("Stopping container...");
    container.stop().await.unwrap();

    log::info!("Creating entity when RPC is down... It should fail.");
    let result = client
        .create_entry(account, Create::from_string("Hello 2", 100))
        .await;
    assert!(result.is_err());

    log::info!("Restarting container...");
    container.restart().await.unwrap();

    log::info!("Creating entity after RPC restart... It should succeed.");
    let create = Create::from_string("Hello 3", 100);
    let result = client.create_entry(account, create).await.unwrap();
    log::info!("Created entity {result}...");

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_transaction_no_rpc_available() -> anyhow::Result<()> {
    init_logger(false);

    let container = ArkivContainer::new(Config::default()).await?;
    let client = ArkivClient::new(container.get_url()?)?;
    let account = create_test_account(&client).await.unwrap();

    let create = Create::from_string("Hello, GolemBase!", 100);
    let result = client.create_entry(account, create).await.unwrap();
    log::info!("Created first entity {result}...");

    // Stop the container to simulate RPC downtime
    container.stop().await.unwrap();

    log::info!("Test must ensure, that creating entity call won't hang in case of RPC downtime.");
    log::info!("Creating entity when RPC is down... It should fail.");
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        client.create_entry(account, Create::from_string("Hello 2", 100)),
    )
    .await
    .expect("Call timed out - function should have failed internally before our timeout");
    assert!(result.is_err());

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_transaction_wrong_chain_id() -> anyhow::Result<()> {
    init_logger(false);

    let mock = ArkivMockServer::new()
        .with_chain_id(5555)
        .default_start()
        .await?;

    // Create client with a different chain ID (137 for Polygon)
    let client = ArkivClient::new(mock.url().clone())?.override_config(TransactionConfig {
        chain_id: Some(137), // Wrong chain ID - mock returns 5555, but we configure 137
        ..TransactionConfig::default()
    });

    // Attempting to create an account with wrong chain ID should fail
    let result = client.account_generate("test123").await;
    assert!(result.is_err());

    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("Chain ID mismatch"));
    log::info!(
        "✅ Correctly rejected account creation with wrong chain ID: {}",
        error_msg
    );

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_transaction_chain_id_change() -> anyhow::Result<()> {
    init_logger(false);

    // Create mock server with chain ID 5555
    let mock = ArkivMockServer::new()
        .with_chain_id(5555)
        .default_start()
        .await?;

    // Create client with correct chain ID initially
    let client = ArkivClient::new(mock.url().clone())?.override_config(TransactionConfig {
        chain_id: Some(5555), // Correct chain ID initially
        ..TransactionConfig::default()
    });

    // Account creation should succeed with correct chain ID
    let account = client.account_generate("test123").await?;
    log::info!("✅ Successfully created account with correct chain ID: {account}");

    // First transaction should succeed
    let create = Create::from_string("Hello, GolemBase!", 100);
    let result = client.create_entry(account, create).await?;
    log::info!("✅ Successfully created first entity {result} with correct chain ID");

    // Now simulate chain ID change by changing the mock server's chain ID.
    // This could be an attack attempting to redirect traffic to a different chain.
    mock.state.set_chain_id(9999);

    // Attempting to send another transaction should fail due to chain ID mismatch
    let create2 = Create::from_string("Hello again!", 100);
    let result2 = client.create_entry(account, create2).await;
    assert!(result2.is_err());

    let error_msg = result2.unwrap_err().to_string();
    assert!(error_msg.contains("chainId does not match node's "));
    log::info!("✅ Correctly rejected transaction after chain ID change: {error_msg}");

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_transaction_stacked_pending() -> anyhow::Result<()> {
    init_logger(false);

    let mock = ArkivMockServer::create_test_mock_server().await?;
    let client = ArkivClient::new(mock.url().clone())?.override_config(TransactionConfig {
        required_confirmations: 1,
        pending_transaction_timeout: Duration::from_secs(3),
        transaction_receipt_timeout: Duration::from_secs(20),
        ..TransactionConfig::default()
    });
    let account = create_test_account(&client).await.unwrap();

    mock.transaction_pool()
        .hold_transactions_for(Duration::from_secs(20))
        .await;

    let create = Create::from_string("E1", 100);
    let result = client.create_entry(account, create).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("timed out"));
    log::info!("First entity creation timed out");

    log::info!("Second entity will wait for previous pending transaction and will send a new one afterwards.");
    let create = Create::from_string("E2", 100);
    let result = client.create_entry(account, create).await.unwrap();
    log::info!("Created second entity {result}...");

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_transaction_stacked_pending_for_infinity() -> anyhow::Result<()> {
    init_logger(false);

    let mock = ArkivMockServer::create_test_mock_server().await?;
    let client = ArkivClient::new(mock.url().clone())?.override_config(TransactionConfig {
        required_confirmations: 1,
        pending_transaction_timeout: Duration::from_secs(3),
        transaction_receipt_timeout: Duration::from_secs(17),
        ..TransactionConfig::default()
    });
    let account = create_test_account(&client).await.unwrap();

    mock.transaction_pool()
        .hold_transactions_for(Duration::from_secs(120))
        .await;

    let create = Create::from_string("E1", 100);
    let result = client.create_entry(account, create).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("timed out"));
    log::info!("First entity creation timed out");

    log::info!("Second transaction should be rejected due to still pending transaction.");
    let create = Create::from_string("E2", 100);
    let result = client.create_entry(account, create).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("pending"));

    Ok(())
}
