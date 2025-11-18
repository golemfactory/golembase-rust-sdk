use alloy::primitives::{Address, B256, U256};
use alloy::rlp::Decodable;
use anyhow;
use derive_more::Debug;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

use arkiv_sdk::account::ARKIV_STORAGE_PROCESSOR_ADDRESS;
use arkiv_sdk::entity::ArkivTransaction;
use arkiv_sdk::utils::wei_to_eth;

use crate::block::{Block, Transaction};
use crate::block_builder::BlockBuilder;
use crate::entity_db::{Entity, EntityDb};
use crate::events::EntityEventHandler;

/// Chain configuration containing chain ID
#[derive(Debug, Clone)]
pub struct ChainConfig {
    pub chain_id: u64,
}

impl ChainConfig {
    pub fn new(chain_id: u64) -> Self {
        Self { chain_id }
    }
}

/// Represents an account in the mock blockchain
#[derive(Clone, Debug)]
pub struct Account {
    pub address: Address,
    pub nonce: U256,
    pub balance: U256,
    pub transactions: Vec<B256>,
    pub received_transactions: Vec<B256>,
}

impl Account {
    /// Create a new empty account
    pub fn new(address: Address) -> Self {
        Self {
            address,
            nonce: U256::ZERO,
            balance: U256::ZERO,
            transactions: Vec::new(),
            received_transactions: Vec::new(),
        }
    }

    /// Create a new empty account (alias for new)
    pub fn empty(address: Address) -> Self {
        Self::new(address)
    }
}

/// Internal state of the blockchain
#[derive(Clone, Debug, Default)]
struct BlockchainState {
    blocks_by_number: HashMap<u64, Arc<Block>>,
    blocks_by_hash: HashMap<B256, Arc<Block>>,
    transactions: HashMap<B256, Arc<Transaction>>,
    accounts: HashMap<Address, Account>,
}

/// Main mock blockchain structure
#[derive(Clone, Debug)]
pub struct Blockchain {
    state: Arc<RwLock<BlockchainState>>,
    entity_db: EntityDb,
    pub config: Arc<Mutex<ChainConfig>>,
    #[debug(ignore)]
    event_handler: Arc<dyn EntityEventHandler>,
}

impl Blockchain {
    /// Create a new empty mock blockchain with an event handler
    pub fn new(
        entity_db: EntityDb,
        event_handler: Arc<dyn EntityEventHandler>,
        chain_id: u64,
    ) -> Self {
        Self {
            state: Arc::new(RwLock::new(BlockchainState::default())),
            entity_db,
            config: Arc::new(Mutex::new(ChainConfig::new(chain_id))),
            event_handler,
        }
    }

    /// Validate transaction nonce
    pub async fn validate_transaction_nonce(
        &self,
        transaction: &Transaction,
    ) -> anyhow::Result<()> {
        let state = self.state.read().await;
        let sender_account = state.accounts.get(&transaction.from);

        let expected_nonce = if let Some(account) = sender_account {
            account.nonce
        } else {
            U256::ZERO
        };

        if U256::from(transaction.nonce) < expected_nonce {
            return Err(anyhow::anyhow!(
                "nonce too low: next nonce {}, tx nonce {}",
                expected_nonce,
                transaction.nonce
            ));
        }

        Ok(())
    }

    /// Process expired entity removal transaction and remove entities
    async fn process_expired_entities(
        &self,
        transaction: &Arc<Transaction>,
        block_builder: &mut BlockBuilder,
    ) -> anyhow::Result<()> {
        // Validate this is a housekeeping transaction
        if transaction.to != ARKIV_STORAGE_PROCESSOR_ADDRESS {
            anyhow::bail!("Transaction is not sent to housekeeping processor address");
        }

        // Validate transaction data is not empty
        if transaction.data.is_empty() {
            anyhow::bail!("Housekeeping transaction has empty data");
        }

        // Decode the entity IDs from the transaction data
        let expired_ids = Vec::<B256>::decode(&mut transaction.data.as_ref()).map_err(|e| {
            anyhow::anyhow!("Failed to decode entity IDs from housekeeping transaction data: {e}")
        })?;

        log::debug!(
            "Processing {} expired entities for removal",
            expired_ids.len()
        );

        // Remove the entities and create individual logs for each
        for entity_id in expired_ids {
            if let Some(entity) = self.entity_db.remove_entity(&entity_id).await {
                block_builder.log_entity_expired(transaction, &entity).await;
            } else {
                log::warn!("Entity 0x{entity_id:x} not found for removal");
            }
        }

        Ok(())
    }

    /// Add a block to the blockchain
    pub async fn add_block(&self, block: Block) {
        let mut state = self.state.write().await;
        let block_number = block.header.block_number;
        let block_hash = block.header.block_hash;
        let mut builder = BlockBuilder::new(self.event_handler.clone(), block);

        // First transaction is always the housekeeping transaction for expired entities
        let transactions = builder.block.transactions.clone();
        if !transactions.is_empty() {
            if let Err(e) = self
                .process_expired_entities(&transactions[0], &mut builder)
                .await
            {
                log::error!("Failed to process expired entities: {e}");
            }

            // Process all transactions in the block
            for transaction in transactions[1..].iter() {
                let transaction_hash = transaction.hash;

                // Add transaction to transactions map
                state
                    .transactions
                    .insert(transaction_hash, transaction.clone());

                // Update accounts
                Self::update_account_for_transaction(&mut state, &transaction);

                // Extract and add entities from transaction data, collect logs
                if let Err(e) = self
                    .extract_entity_from_transaction(transaction, &mut builder)
                    .await
                {
                    log::error!("Failed to extract entities from transaction: {e}");
                }
            }
        }

        let block = builder.build();
        state.blocks_by_number.insert(block_number, block.clone());
        state.blocks_by_hash.insert(block_hash, block.clone());

        // Finish processing the block and emit all collected events.
        // At this point Blockchain must be ready to return block if caller asks,
        // so emit events outside of lock and never move this earlier
        drop(state);
        self.event_handler.finish_block(block_number).await;
    }

    /// Update account state based on a transaction
    fn update_account_for_transaction(state: &mut BlockchainState, transaction: &Arc<Transaction>) {
        // Update sender account
        let sender = transaction.from;
        let sender_account = state
            .accounts
            .entry(sender)
            .or_insert_with(|| Account::new(sender));
        sender_account.nonce += U256::from(1);
        sender_account.balance = sender_account.balance.saturating_sub(transaction.value);
        sender_account.transactions.push(transaction.hash);

        // Update receiver account
        let receiver = transaction.to;
        let receiver_account = state
            .accounts
            .entry(receiver)
            .or_insert_with(|| Account::new(receiver));
        receiver_account.balance += transaction.value;
        receiver_account
            .received_transactions
            .push(transaction.hash);

        if transaction.value > U256::ZERO {
            log::debug!(
                "Account transfer: {} -> {} (value: {} ETH, tx: 0x{:x})",
                transaction.from,
                transaction.to,
                wei_to_eth(transaction.value),
                transaction.hash
            );
        }
    }

    /// Extract entity from transaction data and modify entity database state
    /// Checks if transaction is to storage contract and decodes GolemBase entity operations
    /// Modifies the block using BlockBuilder
    async fn extract_entity_from_transaction(
        &self,
        transaction: &Arc<Transaction>,
        builder: &mut BlockBuilder,
    ) -> anyhow::Result<()> {
        // Check if transaction is to the GolemBase storage processor contract
        if transaction.to != ARKIV_STORAGE_PROCESSOR_ADDRESS {
            return Ok(());
        }

        // Try to decode the transaction data as an ArkivTransaction
        // This is the inverse of the encoding shown in send_db_transaction
        match ArkivTransaction::decode_compressed(transaction.data.as_ref()) {
            Ok(arkiv_tx) => {
                // Process creates
                for (idx, create) in arkiv_tx.creates.into_iter().enumerate() {
                    let entity = Entity::create(create, transaction.from).with_hash(
                        builder.block.header.block_number,
                        idx,
                        transaction.hash,
                    );
                    self.entity_db.add_entity(entity.clone()).await;

                    // Add log and emit event using BlockBuilder
                    builder.log_entity_created(transaction, &entity).await;
                }

                // Process updates
                for update in &arkiv_tx.updates {
                    let entity_key = update.entity_key;

                    let old_entity =
                        self.entity_db
                            .get_entity(&entity_key)
                            .await
                            .ok_or(anyhow::anyhow!(
                                "Entity 0x{entity_key:x} not found for update (tx: 0x{hash:x})",
                                hash = transaction.hash
                            ))?;

                    let updated_entity = self
                        .entity_db
                        .update_entity(&entity_key, update, builder.block.header.block_number)
                        .await
                        .ok_or(anyhow::anyhow!(
                            "Failed to update entity 0x{entity_key:x} (tx: 0x{hash:x})",
                            hash = transaction.hash
                        ))?;

                    let old_expiration = old_entity.expires_at.ok_or(anyhow::anyhow!(
                        "Entity 0x{entity_key:x} missing expiration before update (tx: 0x{hash:x})",
                        hash = transaction.hash
                    ))?;
                    let new_expiration = updated_entity.expires_at.ok_or(anyhow::anyhow!(
                        "Entity 0x{entity_key:x} missing expiration after update (tx: 0x{hash:x})",
                        hash = transaction.hash
                    ))?;
                    builder
                        .log_entity_updated(
                            transaction,
                            &updated_entity,
                            old_expiration,
                            new_expiration,
                        )
                        .await;
                }

                // Process extensions
                for extend in &arkiv_tx.extensions {
                    let entity_key = extend.entity_key;
                    let number_of_blocks = extend.number_of_blocks;

                    let old_entity =
                        self.entity_db
                            .get_entity(&entity_key)
                            .await
                            .ok_or(anyhow::anyhow!(
                                "Entity 0x{entity_key:x} not found for extension (tx: 0x{hash:x})",
                                hash = transaction.hash
                            ))?;

                    let updated_entity = self
                        .entity_db
                        .update_entity_btl(&entity_key, number_of_blocks)
                        .await
                        .ok_or(anyhow::anyhow!(
                            "Failed to extend entity 0x{entity_key:x} (tx: 0x{hash:x})",
                            hash = transaction.hash
                        ))?;

                    let old_expiration = old_entity.expires_at.ok_or(anyhow::anyhow!(
                        "Entity 0x{entity_key:x} missing expiration before extension (tx: 0x{hash:x})",
                        hash = transaction.hash
                    ))?;
                    let new_expiration = updated_entity.expires_at.ok_or(anyhow::anyhow!(
                        "Entity 0x{entity_key:x} missing expiration after extension (tx: 0x{hash:x})",
                        hash = transaction.hash
                    ))?;
                    builder.log_entity_ttl_extended(
                        transaction,
                        entity_key,
                        old_expiration,
                        new_expiration,
                    );
                }

                // Process deletes
                for delete in &arkiv_tx.deletes {
                    let key = *delete;
                    if let Some(entity) = self.entity_db.remove_entity(&key).await {
                        // Add log and emit event
                        builder.log_entity_removed(transaction, &entity).await;
                    } else {
                        log::warn!(
                            "Entity not found for deletion: 0x{:x}, tx: 0x{:x}",
                            key,
                            transaction.hash
                        );
                    }
                }

                // Process change owners
                for change_owner in &arkiv_tx.change_owners {
                    let entity_key = change_owner.entity_key;

                    self.entity_db
                        .get_entity(&entity_key)
                        .await
                        .ok_or(anyhow::anyhow!(
                            "Entity 0x{entity_key:x} not found for owner change (tx: 0x{hash:x})",
                            hash = transaction.hash
                        ))?;

                    let updated_entity = self
                        .entity_db
                        .change_owner(&entity_key, change_owner.new_owner)
                        .await
                        .ok_or(anyhow::anyhow!(
                            "Failed to change owner for entity 0x{entity_key:x} (tx: 0x{hash:x})",
                            hash = transaction.hash
                        ))?;

                    builder
                        .log_entity_owner_changed(transaction, &updated_entity)
                        .await;
                }
            }
            Err(e) => {
                log::warn!("Failed to decode GolemBase transaction: {}", e);
            }
        }

        Ok(())
    }

    /// Get a block by its number
    pub async fn get_block_by_number(&self, block_number: u64) -> Option<Arc<Block>> {
        self.state
            .read()
            .await
            .blocks_by_number
            .get(&block_number)
            .cloned()
    }

    /// Get a block by its hash
    pub async fn get_block_by_hash(&self, block_hash: &B256) -> Option<Arc<Block>> {
        self.state
            .read()
            .await
            .blocks_by_hash
            .get(block_hash)
            .cloned()
    }

    /// Get a transaction by its hash
    pub async fn get_transaction(&self, transaction_hash: &B256) -> Option<Arc<Transaction>> {
        self.state
            .read()
            .await
            .transactions
            .get(transaction_hash)
            .cloned()
    }

    /// Find the block that contains a specific transaction
    pub async fn find_block_containing_transaction(
        &self,
        transaction_hash: &B256,
    ) -> Option<Arc<Block>> {
        let state = self.state.read().await;
        for (_, block) in &state.blocks_by_number {
            for tx in &block.transactions {
                if tx.hash == *transaction_hash {
                    return Some(block.clone());
                }
            }
        }
        None
    }

    /// Get an account by its address
    pub async fn get_account(&self, address: &Address) -> Option<Account> {
        self.state.read().await.accounts.get(address).cloned()
    }

    /// Get a mutable reference to an account by its address
    pub async fn get_account_mut(&self, address: &Address) -> Option<Account> {
        // For now, return a clone since we can't return a mutable reference from async
        self.state.read().await.accounts.get(address).cloned()
    }

    /// Get balance for an account
    pub async fn get_balance(&self, address: &Address) -> U256 {
        self.state
            .read()
            .await
            .accounts
            .get(address)
            .map(|account| account.balance)
            .unwrap_or(U256::ZERO)
    }

    /// Get nonce for an account
    pub async fn get_nonce(&self, address: &Address) -> U256 {
        self.state
            .read()
            .await
            .accounts
            .get(address)
            .map(|account| account.nonce)
            .unwrap_or(U256::ZERO)
    }

    /// Get all accounts
    pub async fn get_accounts(&self) -> Vec<Address> {
        self.state.read().await.accounts.keys().cloned().collect()
    }

    /// Get all blocks
    pub async fn get_blocks(&self) -> HashMap<u64, Arc<Block>> {
        self.state.read().await.blocks_by_number.clone()
    }

    /// Get the latest block number
    pub async fn get_latest_block_number(&self) -> anyhow::Result<u64> {
        let latest = self
            .state
            .read()
            .await
            .blocks_by_number
            .keys()
            .max()
            .copied();

        latest.ok_or_else(|| anyhow::anyhow!("No blocks found in blockchain"))
    }

    /// Add accounts with initial balances
    pub async fn add_accounts(&self, accounts: Vec<Address>) {
        let mut state = self.state.write().await;
        for address in accounts {
            state
                .accounts
                .entry(address)
                .or_insert_with(|| Account::new(address));
        }
    }

    /// Set balance for an account
    pub async fn set_balance(&self, address: Address, balance: U256) {
        let mut state = self.state.write().await;
        let account = state
            .accounts
            .entry(address)
            .or_insert_with(|| Account::new(address));
        account.balance = balance;
    }

    /// Get a reference to the entity database
    pub fn entity_db(&self) -> &EntityDb {
        &self.entity_db
    }

    /// Get the chain ID
    pub fn chain_id(&self) -> u64 {
        self.config.lock().unwrap().chain_id
    }

    /// Set the chain ID
    pub fn set_chain_id(&self, chain_id: u64) {
        self.config.lock().unwrap().chain_id = chain_id;
    }

    /// Validate chain ID against the blockchain's chain ID
    /// Returns an error if the provided chain ID doesn't match
    pub fn validate_chain_id(&self, tx_chain_id: u64) -> anyhow::Result<()> {
        let actual_chain_id = self.chain_id();
        if tx_chain_id != actual_chain_id {
            return Err(anyhow::anyhow!(
                "chainId does not match node's (have={tx_chain_id}, want={actual_chain_id})"
            ));
        }
        Ok(())
    }

    /// Create and add genesis block
    pub async fn create_genesis_block(&self) {
        let genesis_block = Block::new(
            0,                      // Genesis block number
            B256::ZERO,             // No previous block
            Vec::new(),             // No transactions
            U256::from(30_000_000), // Gas limit
            U256::ZERO,             // No gas used
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        );

        self.add_block(genesis_block).await;
    }

    /// Reset blockchain to genesis state: clear all blocks except genesis, clear transactions, reset account nonces
    /// Preserves account balances and account existence
    pub async fn reset_to_genesis(&self) {
        let mut state = self.state.write().await;

        // Get genesis block if it exists
        let genesis_block = state.blocks_by_number.get(&0).cloned();

        // Clear all blocks
        let blocks_cleared = state.blocks_by_number.len();
        state.blocks_by_number.clear();
        state.blocks_by_hash.clear();

        // Clear all transactions
        let transactions_cleared = state.transactions.len();
        state.transactions.clear();

        // Reset all account nonces and transaction lists, but preserve balances
        let accounts_reset = state.accounts.len();
        for account in state.accounts.values_mut() {
            account.nonce = U256::ZERO;
            account.transactions.clear();
            account.received_transactions.clear();
        }

        // Restore genesis block if it existed
        if let Some(genesis) = genesis_block {
            let block_number = genesis.header.block_number;
            let block_hash = genesis.header.block_hash;
            state.blocks_by_number.insert(block_number, genesis.clone());
            state.blocks_by_hash.insert(block_hash, genesis);
        } else {
            // Create new genesis block if none existed
            drop(state);
            self.create_genesis_block().await;
        }

        log::info!(
            "Reset blockchain to genesis: {} blocks cleared, {} transactions cleared, {} accounts reset",
            blocks_cleared,
            transactions_cleared,
            accounts_reset
        );
    }
}
