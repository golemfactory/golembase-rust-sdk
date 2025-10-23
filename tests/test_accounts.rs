use anyhow::Result;
use bigdecimal::BigDecimal;
use arkiv_test_utils::{
    arkiv::{Config, ArkivContainer},
    init_logger,
};
use serial_test::serial;
use std::fs;

use arkiv_sdk::{client::ArkivClient, signers::InMemorySigner, PrivateKeySigner};

const TEST_PRIVATE_KEY_FILE: &str = "test_private.key";

#[tokio::test]
#[serial]
async fn test_account_creation_and_funding() -> Result<()> {
    init_logger(false);

    // Start Arkiv container
    let container = ArkivContainer::new(Config::default()).await?;
    let client = ArkivClient::new(container.get_url()?)?;

    // Create new account
    let account = client.account_generate("test123").await?;
    log::info!("Created new account: {account}");

    // Check initial balance
    let initial_balance = client.get_balance(account).await?;
    log::info!("Initial balance: {initial_balance}");

    // Fund the account
    let fund_amount = BigDecimal::from(1);
    let fund_tx = client.fund(account, fund_amount.clone()).await?;
    log::info!("Account {account} funded with transaction: {fund_tx}");

    // Check final balance
    let final_balance = client.get_balance(account).await?;
    log::info!("Final balance: {final_balance}");

    // Verify balance increased by the funded amount
    assert_eq!(final_balance, initial_balance + fund_amount);
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_account_loading_by_address() -> Result<()> {
    init_logger(false);

    // Start GolemBase container
    let container = ArkivContainer::new(Config::default()).await?;
    let client1 = ArkivClient::new(container.get_url()?)?;

    // Create new account with first client
    let account = client1.account_generate("test123").await?;
    log::info!("Created new account: {account}");

    // Create new client and load account by address
    let client2 = ArkivClient::new(container.get_url()?)?;
    let loaded_account = client2.account_load(account, "test123").await?;
    log::info!("Loaded account by address: {loaded_account}");

    // Verify account was loaded correctly
    assert_eq!(loaded_account, account);
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_account_loading_from_private_key() -> Result<()> {
    init_logger(false);

    // Start Arkiv container
    let container = ArkivContainer::new(Config::default()).await?;
    let client = ArkivClient::new(container.get_url()?)?;

    // Generate a new private key
    let signer = PrivateKeySigner::random();
    let private_key = signer.credential().to_bytes();
    let address = signer.address();
    log::info!("Generated new private key for address: {address}");

    // Get keystore directory and save private key
    let keystore_dir = InMemorySigner::get_keystore_dir()?;
    let private_key_path = keystore_dir.join(TEST_PRIVATE_KEY_FILE);
    log::info!("Saving private key to: {}", private_key_path.display());
    fs::write(&private_key_path, private_key)?;

    // Load account from file
    let loaded_account = client
        .account_load_file(private_key_path.clone(), "")
        .await?;
    log::info!("Loaded account from file: {loaded_account}");

    // Verify account was loaded correctly
    assert_eq!(loaded_account, address);

    // Clean up test private key file
    fs::remove_file(private_key_path)?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_fund_transfer() -> Result<()> {
    init_logger(false);

    // Start Arkiv container
    let container = ArkivContainer::new(Config::default()).await?;
    let client = ArkivClient::new(container.get_url()?)?;

    // Create two accounts
    let account1 = client.account_generate("test123").await?;
    let account2 = client.account_generate("test456").await?;
    log::info!("Created two accounts: {account1} and {account2}");

    // Fund the first account
    let fund_amount = BigDecimal::from(2);
    let fund_tx = client.fund(account1, fund_amount.clone()).await?;
    log::info!("Funded account {account1} with {fund_amount} ETH, tx: {fund_tx}");

    // Check initial balances
    let balance1_before = client.get_balance(account1).await?;
    let balance2_before = client.get_balance(account2).await?;
    log::info!("Initial balances - Account1: {balance1_before}, Account2: {balance2_before}");

    // Transfer funds
    let transfer_amount = BigDecimal::from(1);
    let transfer_tx = client
        .transfer(account1, account2, transfer_amount.clone())
        .await?;
    log::info!(
        "Transferred {transfer_amount} ETH from {account1} to {account2}, tx: {transfer_tx}"
    );

    // Check final balances
    let balance1_after = client.get_balance(account1).await?;
    let balance2_after = client.get_balance(account2).await?;
    log::info!("Final balances - Account1: {balance1_after}, Account2: {balance2_after}");

    let fee_margin = BigDecimal::from(1) / BigDecimal::from(1000);
    assert!(balance1_after >= balance1_before - transfer_amount.clone() - fee_margin);
    assert_eq!(balance2_after, balance2_before + transfer_amount);

    Ok(())
}
