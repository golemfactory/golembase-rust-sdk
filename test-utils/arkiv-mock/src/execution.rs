use crate::block::{Block, Transaction};
use crate::blockchain::Blockchain;
use crate::managed_accounts::ManagedAccounts;
use crate::transaction_pool::TransactionPool;

use alloy::primitives::{B256, U256};
use alloy::rlp::Encodable;
use alloy::rpc::types::TransactionRequest;
use arkiv_sdk::account::ARKIV_STORAGE_PROCESSOR_ADDRESS;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};

/// Configuration for the execution engine
#[derive(Clone, Debug)]
pub struct ExecutionConfig {
    pub block_frequency: Duration,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            block_frequency: Duration::from_secs(2),
        }
    }
}

/// Internal state of the execution engine
#[derive(Clone, Debug, Default)]
struct ExecutionEngineState {
    current_block_number: u64,
    running: bool,
}

/// Execution engine that mines blocks and processes transactions
#[derive(Clone, Debug)]
pub struct ExecutionEngine {
    blockchain: Blockchain,
    transaction_pool: TransactionPool,
    managed_accounts: ManagedAccounts,
    state: Arc<RwLock<ExecutionEngineState>>,
    config: Arc<ExecutionConfig>,
}

impl ExecutionEngine {
    /// Create a new execution engine
    pub fn new(
        blockchain: Blockchain,
        transaction_pool: TransactionPool,
        managed_accounts: ManagedAccounts,
    ) -> Self {
        Self {
            blockchain,
            transaction_pool,
            managed_accounts,
            state: Arc::new(RwLock::new(ExecutionEngineState {
                current_block_number: 0,
                running: false,
            })),
            config: Arc::new(ExecutionConfig::default()),
        }
    }

    /// Start the execution engine
    pub async fn start(&self) {
        {
            let mut state = self.state.write().await;
            if state.running {
                return; // Already running
            }
            state.running = true;
        } // state is automatically dropped here

        let engine = self.clone();
        tokio::spawn(async move {
            engine.run().await;
        });
    }

    /// Stop the execution engine
    pub async fn stop(&self) {
        let mut state = self.state.write().await;
        state.running = false;
    }

    /// Main execution loop
    async fn run(&self) {
        let mut interval = interval(self.config.block_frequency);

        while self.state.read().await.running {
            interval.tick().await;
            self.mine_block().await;
        }
    }

    /// Mine a new block
    async fn mine_block(&self) {
        // Get current block number
        let block_number = {
            let mut state = self.state.write().await;
            state.current_block_number += 1;
            state.current_block_number
        };

        log::info!("=== Mining new block #{block_number} ===");

        // Get transactions from pool (limit to 100 per block)
        let transactions = self.transaction_pool.get_transaction_batch(100).await;
        let transaction_count = transactions.len();

        if transaction_count == 0 {
            log::info!("No transactions in pool, creating empty block");
        } else {
            log::info!("Including {} transactions in block:", transaction_count);
            for (i, tx) in transactions.iter().enumerate() {
                log::info!("  TX {}: 0x{:x}", i + 1, tx.hash);
            }
        }

        let mut transactions = self.filter_invalid_transactions(&transactions).await;
        for transaction in &transactions {
            self.transaction_pool
                .remove_transaction(&transaction.hash)
                .await;
        }

        // Try to add housekeeping transaction (for expired entity removal)
        match self.create_housekeeping_transaction(block_number).await {
            Ok(housekeeping_tx) => {
                transactions.insert(0, housekeeping_tx); // Insert at the beginning
            }
            // Continue without housekeeping transaction
            Err(e) => {
                log::error!(
                    "Failed to create housekeeping transaction for block {block_number}: {e}"
                );
            }
        }

        // Create and add block
        let block = self.create_block(block_number, transactions).await;
        let block_hash = block.header.block_hash;
        self.blockchain.add_block(block).await;

        log::info!(
            "=== Block #{block_number} (0x{:x}) mined successfully with {} transactions ===",
            block_hash,
            transaction_count
        );
    }

    /// Create a transaction for expired entity removal
    async fn create_housekeeping_transaction(
        &self,
        block_number: u64,
    ) -> anyhow::Result<Arc<Transaction>> {
        // Get entity IDs that expire at this block number
        let expiring = self
            .blockchain
            .entity_db()
            .get_entities_expiring_at_block(block_number)
            .await;

        // Get the internal account for housekeeping transactions
        let signer = self.managed_accounts.get_internal_account();
        let signer_address = signer.address();

        // Encode the entity IDs in the data field (always create transaction even if empty)
        let mut data = Vec::new();
        expiring.encode(&mut data);

        // Create a transaction request like in the example
        let transaction_request = TransactionRequest {
            from: Some(signer_address),
            to: Some(alloy::primitives::TxKind::Call(
                ARKIV_STORAGE_PROCESSOR_ADDRESS,
            )),
            value: Some(U256::ZERO),
            gas: Some(0),
            max_fee_per_gas: Some(0u128),
            max_priority_fee_per_gas: Some(0u128),
            input: data.into(),
            nonce: Some(0),
            chain_id: Some(self.blockchain.chain_id()),
            ..Default::default()
        };

        // Build and sign the transaction using unified functions
        let (signature, signed) = Transaction::sign_request(transaction_request.clone(), &signer)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to sign housekeeping transaction: {e}"))?;

        let internal_transaction =
            Transaction::from_signed(transaction_request, &signature, &signed)
                .map_err(|e| anyhow::anyhow!("Failed to convert to internal transaction: {e}"))?;

        Ok(Arc::new(internal_transaction))
    }

    /// Create a block with the given transactions
    async fn create_block(&self, block_number: u64, transactions: Vec<Arc<Transaction>>) -> Block {
        let previous_block_hash = if block_number == 1 {
            B256::ZERO
        } else {
            // Get the real previous block hash from the blockchain
            if let Some(prev_block) = self.blockchain.get_block_by_number(block_number - 1).await {
                prev_block.header.block_hash
            } else {
                // Fallback to a deterministic hash if previous block not found
                B256::from_slice(&[(block_number - 1) as u8; 32])
            }
        };

        let gas_limit = U256::from(30_000_000);
        let gas_used = U256::from(transactions.len() * 21_000); // Simple gas calculation
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Block::new(
            block_number,
            previous_block_hash,
            transactions,
            gas_limit,
            gas_used,
            timestamp,
        )
    }

    /// Filter and order transactions by nonce for inclusion in a block
    /// This handles multiple transactions from the same account with sequential nonces
    async fn filter_invalid_transactions(
        &self,
        transactions: &[Arc<Transaction>],
    ) -> Vec<Arc<Transaction>> {
        let mut result = Vec::new();
        let mut sender_txs = HashMap::new();

        // Group transactions by sender
        for tx in transactions {
            sender_txs
                .entry(tx.from)
                .or_insert_with(Vec::new)
                .push(tx.clone());
        }

        // For each sender, sort transactions by nonce and validate them
        for (sender, txs) in sender_txs {
            // Get current nonce for this sender from blockchain
            let current_nonce = self.blockchain.get_nonce(&sender).await;
            let mut expected_nonce = current_nonce;

            // Sort transactions by nonce
            let mut sorted_txs = txs;
            sorted_txs.sort_by_key(|tx| tx.nonce);

            // Add transactions in nonce order if they're valid
            for tx in sorted_txs {
                if U256::from(tx.nonce) == expected_nonce {
                    // Nonce is correct, add to result and increment expected nonce
                    result.push(tx);
                    expected_nonce += U256::from(1);
                } else {
                    // Nonce is incorrect, log and skip
                    log::warn!(
                        "Transaction 0x{:x} from {} has invalid nonce: expected {}, got {}",
                        tx.hash,
                        sender,
                        expected_nonce,
                        tx.nonce
                    );
                }
            }
        }

        result
    }

    /// Get current block number
    pub async fn get_current_block_number(&self) -> u64 {
        self.state.read().await.current_block_number
    }

    /// Check if the engine is running
    pub async fn is_running(&self) -> bool {
        self.state.read().await.running
    }
}
