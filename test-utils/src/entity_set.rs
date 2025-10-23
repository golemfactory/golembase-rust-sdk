use anyhow::Result;

use alloy::primitives::Address;
use golem_base_sdk::client::GolemBaseClient;
use golem_base_sdk::entity::{Create, Hash};

/// Creates a standard set of test entities for comprehensive testing
pub async fn create_standard_test_entities(
    client: &GolemBaseClient,
    account: Address,
) -> Result<Vec<Hash>> {
    let entity1 = client
        .create_entry(
            account,
            Create::new(b"test_data_1".to_vec(), 1000)
                .annotate_string("type", "test")
                .annotate_string("category", "alpha")
                .annotate_string("status", "active")
                .annotate_number("priority", 1)
                .annotate_number("version", 1),
        )
        .await?;
    let entity2 = client
        .create_entry(
            account,
            Create::new(b"test_data_2".to_vec(), 2000)
                .annotate_string("type", "test")
                .annotate_string("category", "beta")
                .annotate_string("status", "inactive")
                .annotate_number("priority", 2)
                .annotate_number("version", 2),
        )
        .await?;
    let entity3 = client
        .create_entry(
            account,
            Create::new(b"test_data_3".to_vec(), 3000)
                .annotate_string("type", "demo")
                .annotate_string("category", "alpha")
                .annotate_string("status", "active")
                .annotate_number("priority", 3)
                .annotate_number("version", 1),
        )
        .await?;
    let entity4 = client
        .create_entry(
            account,
            Create::new(b"test_data_4".to_vec(), 4000)
                .annotate_string("type", "demo")
                .annotate_string("category", "gamma")
                .annotate_string("status", "pending")
                .annotate_number("priority", 1)
                .annotate_number("version", 3),
        )
        .await?;
    Ok(vec![entity1, entity2, entity3, entity4])
}

/// Creates entities for testing owner-specific queries
pub async fn create_owner_test_entities(
    client: &GolemBaseClient,
    account1: Address,
    account2: Address,
) -> Result<(Hash, Hash, Hash)> {
    let entity1 = client
        .create_entry(
            account1,
            Create::new(b"account1_data_1".to_vec(), 1000).annotate_string("owner", "account1"),
        )
        .await?;
    let entity2 = client
        .create_entry(
            account1,
            Create::new(b"account1_data_2".to_vec(), 1000).annotate_string("owner", "account1"),
        )
        .await?;
    let entity3 = client
        .create_entry(
            account2,
            Create::new(b"account2_data_1".to_vec(), 1000).annotate_string("owner", "account2"),
        )
        .await?;
    Ok((entity1, entity2, entity3))
}

/// Creates entities for testing expiration queries
pub async fn create_expiration_test_entities(
    client: &GolemBaseClient,
    account: Address,
) -> Result<(Hash, Hash, Hash)> {
    let entity1 = client
        .create_entry(
            account,
            Create::new(b"expire_test_1".to_vec(), 1000)
                .annotate_string("expiration_test", "block_1000"),
        )
        .await?;
    let entity2 = client
        .create_entry(
            account,
            Create::new(b"expire_test_2".to_vec(), 2000)
                .annotate_string("expiration_test", "block_2000"),
        )
        .await?;
    let entity3 = client
        .create_entry(
            account,
            Create::new(b"expire_test_3".to_vec(), 3000)
                .annotate_string("expiration_test", "block_3000"),
        )
        .await?;
    Ok((entity1, entity2, entity3))
}
