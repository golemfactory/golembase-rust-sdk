use crate::block::Transaction;
use alloy::primitives::Address;
use alloy::primitives::B256;
use alloy::primitives::U256;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Internal state of the transaction pool wrapped in RwLock
#[derive(Clone, Debug, Default)]
pub struct TransactionPoolState {
    transactions: HashMap<B256, Arc<Transaction>>,
    pool_created_at: Option<Instant>,
    hold_duration: Duration,
}

impl TransactionPoolState {
    /// Create a new empty transaction pool state
    pub fn new() -> Self {
        Self {
            transactions: HashMap::new(),
            pool_created_at: Some(Instant::now()),
            hold_duration: Duration::from_secs(0), // No hold by default
        }
    }

    /// Set the hold duration for the pool
    pub fn hold_transactions_for(&mut self, hold_duration: Duration) {
        self.hold_duration = hold_duration;
        // Reset the pool creation time when hold duration is set
        self.pool_created_at = Some(Instant::now());
    }

    /// Check if the hold period has passed
    pub fn is_hold_period_passed(&self) -> bool {
        if let Some(created_at) = self.pool_created_at {
            let now = Instant::now();
            now.duration_since(created_at) >= self.hold_duration
        } else {
            true // No hold period set
        }
    }

    /// Add a transaction to the pool
    pub fn add_transaction(&mut self, transaction: Arc<Transaction>) {
        let hash = transaction.hash;
        self.transactions.insert(hash, transaction);

        log::info!("Transaction 0x{:x} added to pool", hash);
    }

    /// Get a transaction from the pool by hash
    pub fn get_transaction(&self, hash: &B256) -> Option<Arc<Transaction>> {
        self.transactions.get(hash).cloned()
    }

    /// Remove a transaction from the pool
    pub fn remove_transaction(&mut self, hash: &B256) -> Option<Arc<Transaction>> {
        self.transactions.remove(hash)
    }

    /// Get all transactions
    pub fn get_all_transactions(&self) -> Vec<Arc<Transaction>> {
        self.transactions.values().cloned().collect()
    }

    /// Get a batch of transactions (for mining) - only returns transactions if pool hold period has passed
    pub fn get_transaction_batch(&self, max_count: usize) -> Vec<Arc<Transaction>> {
        if !self.is_hold_period_passed() {
            // Hold period not yet passed, return empty batch
            return Vec::new();
        }

        // Hold period has passed, return transactions
        self.transactions
            .values()
            .take(max_count)
            .cloned()
            .collect()
    }

    /// Get the number of transactions
    pub fn count(&self) -> usize {
        self.transactions.len()
    }

    /// Check if the pool is empty
    pub fn is_empty(&self) -> bool {
        self.transactions.is_empty()
    }

    /// Get transaction count for an address (nonce)
    pub fn get_transaction_count(&self, address: &Address) -> U256 {
        let count = self
            .transactions
            .values()
            .filter(|tx| tx.from == *address)
            .count();
        U256::from(count)
    }
}

/// Transaction pool that holds pending transactions with time-based batching
#[derive(Clone, Debug, Default)]
pub struct TransactionPool {
    state: Arc<RwLock<TransactionPoolState>>,
}

impl TransactionPool {
    /// Create a new empty transaction pool
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(TransactionPoolState::new())),
        }
    }

    /// Set the hold duration for the pool
    pub async fn hold_transactions_for(&self, hold_duration: Duration) {
        self.state
            .write()
            .await
            .hold_transactions_for(hold_duration);
    }

    /// Add a transaction to the pool
    pub async fn add_transaction(&self, transaction: Arc<Transaction>) {
        self.state.write().await.add_transaction(transaction);
    }

    /// Get a transaction from the pool by hash
    pub async fn get_transaction(&self, hash: &B256) -> Option<Arc<Transaction>> {
        self.state.read().await.get_transaction(hash)
    }

    /// Remove a transaction from the pool (when it gets mined)
    pub async fn remove_transaction(&self, hash: &B256) -> Option<Arc<Transaction>> {
        self.state.write().await.remove_transaction(hash)
    }

    /// Get all pending transactions
    pub async fn get_all_transactions(&self) -> Vec<Arc<Transaction>> {
        self.state.read().await.get_all_transactions()
    }

    /// Get a batch of transactions (for mining)
    pub async fn get_transaction_batch(&self, max_count: usize) -> Vec<Arc<Transaction>> {
        self.state.read().await.get_transaction_batch(max_count)
    }

    /// Get the number of pending transactions
    pub async fn count(&self) -> usize {
        self.state.read().await.count()
    }

    /// Check if the pool is empty
    pub async fn is_empty(&self) -> bool {
        self.state.read().await.is_empty()
    }

    /// Get transaction count for an address (nonce)
    pub async fn get_transaction_count(&self, address: &Address) -> U256 {
        self.state.read().await.get_transaction_count(address)
    }

    /// Get transaction receipt (mock implementation)
    pub async fn get_receipt(&self, _hash: &B256) -> Option<serde_json::Value> {
        // Mock implementation - return None for now
        // In a real implementation, this would return actual transaction receipts
        None
    }

    /// Get transaction by hash
    pub async fn get_transaction_by_hash(&self, hash: &B256) -> Option<Arc<Transaction>> {
        // Return the actual transaction object
        self.get_transaction(hash).await
    }
}
