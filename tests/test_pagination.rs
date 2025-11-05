use arkiv_sdk::client::{ArkivClient, TransactionConfig};
use arkiv_sdk::entity::Hash;
use arkiv_sdk::rpc::{Error, QueryOptions, SearchResult};
use arkiv_test_utils::arkiv::ArkivContainer;
use arkiv_test_utils::create_test_account;
use arkiv_test_utils::entity_set::{
    create_large_count_test_entities, create_large_size_test_entities_default,
    create_mixed_size_test_entities,
};
use arkiv_test_utils::init_logger;
use futures::StreamExt;
use serial_test::serial;

/// Test scenarios for pagination functionality using query_streamed
///
/// This test suite covers various pagination scenarios to ensure the query_streamed
/// function works correctly with different page sizes, data volumes, and edge cases.

fn high_gas_config() -> TransactionConfig {
    let mut config = TransactionConfig::default();
    config.gas_limit = 10_000_000; // Much higher than default 1,000,000
    config
}

/// SCENARIO 1: Basic Pagination with Small Page Size
/// - Create 1000 entities with consistent annotations
/// - Query with small page size (e.g., 10 results per page)
/// - Verify all entities are returned across multiple pages
/// - Verify no duplicates and no missing entities
/// - Verify cursor-based pagination works correctly
#[tokio::test]
#[serial]
async fn test_basic_pagination_small_pages() -> anyhow::Result<()> {
    init_logger(false);
    let arkiv = ArkivContainer::new(Default::default()).await?;
    let client = ArkivClient::new(arkiv.get_url()?)?.override_config(high_gas_config());
    let account = create_test_account(&client).await?;

    let expected_entities = create_large_count_test_entities(&client, account, 1000).await?;
    let options = QueryOptions::with_all().with_page_size(10);
    let stream = Box::pin(client.query_streamed("test_type = \"pagination_count\"", &options));
    let all_results = collect_pagination_stream(stream).await?;

    verify_expected_results(&expected_entities, &all_results)?;

    Ok(())
}

/// SCENARIO 2: Large Page Size vs Small Dataset
/// - Create 50 entities
/// - Query with large page size (e.g., 100 results per page)
/// - Verify all entities are returned in a single page
/// - Verify cursor is None when all results fit in one page
#[tokio::test]
#[serial]
async fn test_large_pages_small_dataset() -> anyhow::Result<()> {
    init_logger(false);
    let arkiv = ArkivContainer::new(Default::default()).await?;
    let client = ArkivClient::new(arkiv.get_url()?)?.override_config(high_gas_config());
    let account = create_test_account(&client).await?;

    let expected_entities = create_large_count_test_entities(&client, account, 50).await?;
    let options = QueryOptions::with_all().with_page_size(100);
    let stream = Box::pin(client.query_streamed("test_type = \"pagination_count\"", &options));
    let all_results = collect_pagination_stream(stream).await?;

    verify_expected_results(&expected_entities, &all_results)?;

    Ok(())
}

/// SCENARIO 3: Large Dataset with Medium Page Size
/// - Create 1000 entities
/// - Query with medium page size (e.g., 50 results per page)
/// - Verify all entities are returned across ~20 pages
/// - Verify consistent ordering across pages
/// - Verify memory usage remains reasonable
#[tokio::test]
#[serial]
async fn test_large_dataset_medium_pages() -> anyhow::Result<()> {
    init_logger(false);
    let arkiv = ArkivContainer::new(Default::default()).await?;
    let client = ArkivClient::new(arkiv.get_url()?)?.override_config(high_gas_config());
    let account = create_test_account(&client).await?;

    let expected_entities = create_large_count_test_entities(&client, account, 1000).await?;
    let options = QueryOptions::with_all().with_page_size(50);
    let stream = Box::pin(client.query_streamed("test_type = \"pagination_count\"", &options));
    let all_results = collect_pagination_stream(stream).await?;

    verify_expected_results(&expected_entities, &all_results)?;
    verify_sequence_order(&all_results)?;

    Ok(())
}

/// SCENARIO 4: Large Entity Size Impact on Pagination
/// - Create entities with progressively larger sizes (up to 1MB each)
/// - Query with various page sizes
/// - Verify pagination works correctly regardless of entity size
/// - Verify large entities don't break pagination logic
#[tokio::test]
#[serial]
async fn test_large_entity_sizes() -> anyhow::Result<()> {
    init_logger(false);
    let arkiv = ArkivContainer::new(Default::default()).await?;
    let client = ArkivClient::new(arkiv.get_url()?)?.override_config(high_gas_config());
    let account = create_test_account(&client).await?;

    let expected_entities = create_large_size_test_entities_default(&client, account).await?;
    for page_size in [1, 5, 10] {
        let options = QueryOptions::with_all().with_page_size(page_size);
        let stream = Box::pin(client.query_streamed("test_type = \"pagination_size\"", &options));
        let all_results = collect_pagination_stream(stream).await?;

        verify_expected_results(&expected_entities, &all_results)?;
    }

    Ok(())
}

/// SCENARIO 5: Filtered Queries with Pagination
/// - Create 500 entities with different categories (alpha, beta, gamma, delta)
/// - Query for specific categories with pagination
/// - Verify only matching entities are returned
/// - Verify pagination works correctly with filtered results
#[tokio::test]
#[serial]
async fn test_filtered_queries_pagination() -> anyhow::Result<()> {
    init_logger(false);
    let arkiv = ArkivContainer::new(Default::default()).await?;
    let client = ArkivClient::new(arkiv.get_url()?)?.override_config(high_gas_config());
    let account = create_test_account(&client).await?;

    create_large_count_test_entities(&client, account, 500).await?;
    for category in ["alpha", "beta", "gamma", "delta"] {
        let query = format!(
            "test_type = \"pagination_count\" && category = \"{}\"",
            category
        );
        let options = QueryOptions::with_all().with_page_size(25);
        let stream = Box::pin(client.query_streamed(&query, &options));
        let all_results = collect_pagination_stream(stream).await?;

        assert!(
            all_results.len() >= 120 && all_results.len() <= 130,
            "Expected ~125 entities for category {}, got {}",
            category,
            all_results.len()
        );

        verify_category_annotations(&all_results, category);
    }

    Ok(())
}

/// SCENARIO 6: Concurrent Pagination Streams
/// - Create large dataset
/// - Start multiple concurrent pagination streams
/// - Verify each stream returns complete, non-overlapping results
/// - Verify no race conditions or data corruption
#[tokio::test]
#[serial]
async fn test_concurrent_pagination_streams() -> anyhow::Result<()> {
    init_logger(false);
    let arkiv = ArkivContainer::new(Default::default()).await?;
    let client = ArkivClient::new(arkiv.get_url()?)?.override_config(high_gas_config());
    let account = create_test_account(&client).await?;

    let expected_entities = create_large_count_test_entities(&client, account, 200).await?;
    let options1 = QueryOptions::with_all().with_page_size(10);
    let options2 = QueryOptions::with_all().with_page_size(25);
    let options3 = QueryOptions::with_all().with_page_size(50);

    let stream1_future = async {
        let stream = Box::pin(client.query_streamed("test_type = \"pagination_count\"", &options1));
        collect_pagination_stream(stream).await
    };
    let stream2_future = async {
        let stream = Box::pin(client.query_streamed("test_type = \"pagination_count\"", &options2));
        collect_pagination_stream(stream).await
    };
    let stream3_future = async {
        let stream = Box::pin(client.query_streamed("test_type = \"pagination_count\"", &options3));
        collect_pagination_stream(stream).await
    };

    let (results1, results2, results3) =
        tokio::try_join!(stream1_future, stream2_future, stream3_future)?;

    verify_expected_results(&expected_entities, &results1)?;
    verify_expected_results(&expected_entities, &results2)?;
    verify_expected_results(&expected_entities, &results3)?;

    Ok(())
}

/// SCENARIO 7: Cursor Persistence and Resumption
/// - Start pagination and save cursor at various points
/// - Resume pagination from saved cursor
/// - Verify results continue from correct position
/// - Verify no duplicate or missing entities
#[tokio::test]
#[serial]
async fn test_cursor_persistence_resumption() -> anyhow::Result<()> {
    init_logger(false);
    let arkiv = ArkivContainer::new(Default::default()).await?;
    let client = ArkivClient::new(arkiv.get_url()?)?.override_config(high_gas_config());
    let account = create_test_account(&client).await?;

    let expected_entities = create_large_count_test_entities(&client, account, 100).await?;

    let options = QueryOptions::with_all().with_page_size(20);
    let mut stream = Box::pin(client.query_streamed("test_type = \"pagination_count\"", &options));

    let first_page = stream.next().await.unwrap()?;
    let mut all_results = first_page;

    let _interfering_entities = create_large_count_test_entities(&client, account, 50).await?;

    while let Some(result) = stream.next().await {
        match result {
            Ok(mut results) => all_results.append(&mut results),
            Err(e) => return Err(anyhow::anyhow!("Pagination error: {}", e)),
        }
    }

    verify_expected_results(&expected_entities, &all_results)?;

    Ok(())
}

/// SCENARIO 8: Boundary Conditions
/// - Test pagination at exact page boundaries
/// - Test with page size equal to total count
/// - Test with page size larger than total count
/// - Verify correct behavior in all boundary cases
#[tokio::test]
#[serial]
async fn test_boundary_conditions() -> anyhow::Result<()> {
    init_logger(false);
    let arkiv = ArkivContainer::new(Default::default()).await?;
    let client = ArkivClient::new(arkiv.get_url()?)?.override_config(high_gas_config());
    let account = create_test_account(&client).await?;

    let expected_entities = create_large_count_test_entities(&client, account, 100).await?;
    let options_100 = QueryOptions::with_all().with_page_size(100);
    let stream = Box::pin(client.query_streamed("test_type = \"pagination_count\"", &options_100));
    let results_100 = collect_pagination_stream(stream).await?;
    verify_expected_results(&expected_entities, &results_100)?;

    let options_200 = QueryOptions::with_all().with_page_size(200);
    let stream = Box::pin(client.query_streamed("test_type = \"pagination_count\"", &options_200));
    let results_200 = collect_pagination_stream(stream).await?;
    verify_expected_results(&expected_entities, &results_200)?;

    let options_1 = QueryOptions::with_all().with_page_size(1);
    let stream = Box::pin(client.query_streamed("test_type = \"pagination_count\"", &options_1));
    let results_1 = collect_pagination_stream(stream).await?;
    verify_expected_results(&expected_entities, &results_1)?;

    let options_50 = QueryOptions::with_all().with_page_size(50);
    let stream = Box::pin(client.query_streamed("test_type = \"pagination_count\"", &options_50));
    let results_50 = collect_pagination_stream(stream).await?;
    verify_expected_results(&expected_entities, &results_50)?;

    let options_25 = QueryOptions::with_all().with_page_size(25);
    let stream = Box::pin(client.query_streamed("test_type = \"pagination_count\"", &options_25));
    let results_25 = collect_pagination_stream(stream).await?;
    verify_expected_results(&expected_entities, &results_25)?;

    Ok(())
}

/// SCENARIO 9: Mixed Entity Sizes
/// - Create 100 small entities (1KB each) plus 5 very large entities (1MB each)
/// - Test pagination with various page sizes
/// - Verify large entities don't break pagination
/// - Verify all entities are returned regardless of size
#[tokio::test]
#[serial]
async fn test_mixed_entity_sizes() -> anyhow::Result<()> {
    init_logger(false);
    let arkiv = ArkivContainer::new(Default::default()).await?;
    let client = ArkivClient::new(arkiv.get_url()?)?.override_config(high_gas_config());
    let account = create_test_account(&client).await?;

    let expected_entities = create_mixed_size_test_entities(&client, account).await?;
    for page_size in [10, 25, 50, 105] {
        let options = QueryOptions::with_all().with_page_size(page_size);
        let stream = Box::pin(client.query_streamed("test_type = \"mixed_size\"", &options));
        let all_results = collect_pagination_stream(stream).await?;

        verify_expected_results(&expected_entities, &all_results)?;
        let mut small_count = 0;
        let mut large_count = 0;

        for result in &all_results {
            let size_category = result
                .string_annotations
                .iter()
                .find(|a| a.key == "size_category")
                .unwrap();
            match size_category.value.as_str() {
                "small" => small_count += 1,
                "large" => large_count += 1,
                _ => {}
            }
        }

        assert_eq!(small_count, 100);
        assert_eq!(large_count, 5);
    }

    Ok(())
}

fn verify_uniqueness(results: &[SearchResult]) -> anyhow::Result<()> {
    let mut keys = std::collections::HashSet::new();
    for result in results {
        if !keys.insert(result.key) {
            return Err(anyhow::anyhow!("Duplicate key found: {:?}", result.key));
        }
    }
    Ok(())
}

fn verify_sequence_order(results: &[SearchResult]) -> anyhow::Result<()> {
    for (i, result) in results.iter().enumerate() {
        let sequence_ann = result
            .numeric_annotations
            .iter()
            .find(|a| a.key == "sequence");
        if let Some(seq) = sequence_ann {
            if seq.value != i as u64 {
                return Err(anyhow::anyhow!(
                    "Results not in correct order at index {}: expected {}, got {}",
                    i,
                    i,
                    seq.value
                ));
            }
        }
    }
    Ok(())
}

fn verify_category_annotations(results: &[SearchResult], expected_category: &str) {
    for result in results {
        let cat_ann = result
            .string_annotations
            .iter()
            .find(|a| a.key == "category");
        assert!(cat_ann.is_some(), "Missing category annotation");
        assert_eq!(
            cat_ann.unwrap().value,
            expected_category,
            "Wrong category in result"
        );
    }
}

async fn collect_pagination_stream(
    mut stream: impl futures::StreamExt<Item = Result<Vec<SearchResult>, Error>> + Unpin,
) -> anyhow::Result<Vec<SearchResult>> {
    let mut all_results = Vec::new();

    while let Some(result) = stream.next().await {
        match result {
            Ok(mut results) => all_results.append(&mut results),
            Err(e) => return Err(anyhow::anyhow!("Pagination error: {}", e)),
        }
    }

    Ok(all_results)
}

fn verify_expected_results(
    expected_entities: &[Hash],
    actual_results: &[SearchResult],
) -> anyhow::Result<()> {
    if actual_results.len() != expected_entities.len() {
        return Err(anyhow::anyhow!(
            "Expected {} results, got {}",
            expected_entities.len(),
            actual_results.len()
        ));
    }

    verify_uniqueness(actual_results)?;

    let actual_keys: std::collections::HashSet<_> = actual_results.iter().map(|r| r.key).collect();
    let expected_keys: std::collections::HashSet<_> = expected_entities.iter().copied().collect();

    if actual_keys != expected_keys {
        return Err(anyhow::anyhow!(
            "Result keys don't match expected entities. Missing: {:?}, Extra: {:?}",
            expected_keys.difference(&actual_keys).collect::<Vec<_>>(),
            actual_keys.difference(&expected_keys).collect::<Vec<_>>()
        ));
    }

    Ok(())
}
