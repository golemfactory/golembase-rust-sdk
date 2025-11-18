use anyhow::Result;

use alloy::primitives::Address;
use arkiv_sdk::client::ArkivClient;
use arkiv_sdk::entity::{Create, Hash};

/// Creates a standard set of test entities for comprehensive testing
pub async fn create_standard_test_entities(
    client: &ArkivClient,
    account: Address,
) -> Result<Vec<Hash>> {
    let entity1 = client
        .create_entry(
            account,
            Create::text("test_data_1", 1000)
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
            Create::text("test_data_2", 2000)
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
            Create::text("test_data_3", 3000)
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
            Create::text("test_data_4", 4000)
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
    client: &ArkivClient,
    account1: Address,
    account2: Address,
) -> Result<(Hash, Hash, Hash)> {
    let entity1 = client
        .create_entry(
            account1,
            Create::text("account1_data_1", 1000).annotate_string("owner", "account1"),
        )
        .await?;
    let entity2 = client
        .create_entry(
            account1,
            Create::text("account1_data_2", 1000).annotate_string("owner", "account1"),
        )
        .await?;
    let entity3 = client
        .create_entry(
            account2,
            Create::text("account2_data_1", 1000).annotate_string("owner", "account2"),
        )
        .await?;
    Ok((entity1, entity2, entity3))
}

/// Creates entities for testing expiration queries
pub async fn create_expiration_test_entities(
    client: &ArkivClient,
    account: Address,
) -> Result<(Hash, Hash, Hash)> {
    let entity1 = client
        .create_entry(
            account,
            Create::text("expire_test_1", 1000).annotate_string("expiration_test", "block_1000"),
        )
        .await?;
    let entity2 = client
        .create_entry(
            account,
            Create::text("expire_test_2", 2000).annotate_string("expiration_test", "block_2000"),
        )
        .await?;
    let entity3 = client
        .create_entry(
            account,
            Create::text("expire_test_3", 3000).annotate_string("expiration_test", "block_3000"),
        )
        .await?;
    Ok((entity1, entity2, entity3))
}

/// Creates a large number of test entities for pagination testing
/// Each entity has a unique identifier and consistent annotations for filtering
pub async fn create_large_count_test_entities(
    client: &ArkivClient,
    account: Address,
    count: usize,
) -> Result<Vec<Hash>> {
    let mut all_entities = Vec::with_capacity(count);
    const BATCH_SIZE: usize = 100; // Small payloads, can batch more

    for batch_start in (0..count).step_by(BATCH_SIZE) {
        let batch_end = std::cmp::min(batch_start + BATCH_SIZE, count);
        let mut creates = Vec::with_capacity(BATCH_SIZE);

        for i in batch_start..batch_end {
            let entry = Create::text(format!("pagination_test_data_{:06}", i), 1000 + i as u64)
                .annotate_string("test_type", "pagination_count")
                .annotate_string(
                    "category",
                    match i % 4 {
                        0 => "alpha",
                        1 => "beta",
                        2 => "gamma",
                        _ => "delta",
                    },
                )
                .annotate_string("status", if i % 2 == 0 { "active" } else { "inactive" })
                .annotate_number("sequence", i as u64)
                .annotate_number("priority", (i % 10) as u64)
                .annotate_number("batch", (i / 100) as u64);
            creates.push(entry);
        }

        let entities = client.create_entries(account, creates).await?;
        all_entities.extend(entities);
    }

    Ok(all_entities)
}

/// Creates a large number of test entities using sensible defaults
/// Creates 100 entities with consistent annotations for pagination testing
pub async fn create_large_count_test_entities_default(
    client: &ArkivClient,
    account: Address,
) -> Result<Vec<Hash>> {
    create_large_count_test_entities(client, account, 100).await
}

/// Creates entities with progressively larger sizes for pagination testing
/// Each entity is approximately 2x the size of the previous one, up to max_size
pub async fn create_large_size_test_entities(
    client: &ArkivClient,
    account: Address,
    count: usize,
    base_size: usize,
    max_size: usize,
) -> Result<Vec<Hash>> {
    let mut entities = Vec::with_capacity(count);

    for i in 0..count {
        // Calculate size: base_size * 2^i, capped at max_size
        let size = std::cmp::min(base_size * (1 << i), max_size);
        let data = vec![b'X'; size];

        let entity = client
            .create_entry(
                account,
                Create::binary(data, 1000 + i as u64)
                    .annotate_string("test_type", "pagination_size")
                    .annotate_string(
                        "size_category",
                        match size {
                            s if s < 1024 => "tiny",
                            s if s < 10240 => "small",
                            s if s < 102400 => "medium",
                            s if s < 1024000 => "large",
                            _ => "xlarge",
                        },
                    )
                    .annotate_number("size_bytes", size as u64)
                    .annotate_number("sequence", i as u64),
            )
            .await?;
        entities.push(entity);
    }

    Ok(entities)
}

/// Creates entities with progressively larger sizes using sensible defaults
/// Creates 10 entities starting from 1KB, doubling each time up to 64KB (transaction size limit)
pub async fn create_large_size_test_entities_default(
    client: &ArkivClient,
    account: Address,
) -> Result<Vec<Hash>> {
    create_large_size_test_entities(client, account, 10, 1024, 64 * 1024).await
}

/// Creates a mixed set of entities with many small ones and a few large ones
/// Creates 100 small entities (1KB each) plus 5 large entities (32KB each)
pub async fn create_mixed_size_test_entities(
    client: &ArkivClient,
    account: Address,
) -> Result<Vec<Hash>> {
    let mut entities = Vec::with_capacity(105);

    // Create 100 small entities (1KB each) in batches to respect transaction size limit
    const BATCH_SIZE: usize = 50; // 50KB per batch, well under 131KB limit
    for batch_start in (0..100).step_by(BATCH_SIZE) {
        let batch_end = std::cmp::min(batch_start + BATCH_SIZE, 100);
        let mut small_creates = Vec::with_capacity(BATCH_SIZE);

        for i in batch_start..batch_end {
            let entry = Create::binary(vec![b'S'; 1024], 1000 + i as u64)
                .annotate_string("test_type", "mixed_size")
                .annotate_string("size_category", "small")
                .annotate_number("size_bytes", 1024)
                .annotate_number("sequence", i as u64)
                .annotate_string("entity_type", "small");
            small_creates.push(entry);
        }
        let small_entities = client.create_entries(account, small_creates).await?;
        entities.extend(small_entities);
    }

    // Create 5 large entities (32KB each) one by one
    for i in 0..5 {
        let entity = client
            .create_entry(
                account,
                Create::binary(vec![b'L'; 32 * 1024], 2000 + i as u64)
                    .annotate_string("test_type", "mixed_size")
                    .annotate_string("size_category", "large")
                    .annotate_number("size_bytes", 32 * 1024)
                    .annotate_number("sequence", 100 + i as u64)
                    .annotate_string("entity_type", "large"),
            )
            .await?;
        entities.push(entity);
    }

    Ok(entities)
}
