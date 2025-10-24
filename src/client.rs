use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use alloy::eips::BlockNumberOrTag;
use alloy::primitives::{Address, B256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::rpc::client::ClientRef;
use alloy::rpc::types::{Log, SyncStatus, TransactionReceipt};
use alloy::signers::local::PrivateKeySigner;
use alloy::transports::http::reqwest::Url;
use bigdecimal::BigDecimal;
use bon::bon;
use bytes::Bytes;
use log;
use tokio::sync::Mutex;

use crate::account::Account;
use crate::entity::{ArkivTransaction, Create, Hash, Update};
use crate::events::{arkiv_storage_entity_created, EventsClient};
use crate::resilient_provider::ResilientProvider;
use crate::rpc::Error;
use crate::signers::{ArkivSigner, InMemorySigner, TransactionSigner};
use crate::utils::wei_to_eth;

/// Tracks and assigns sequential Ethereum nonces for concurrent transactions.
pub struct NonceManager {
    /// Last known on-chain nonce.
    pub base_nonce: u64,
    /// Number of in-flight (pending) transactions.
    pub in_flight: u64,
}

impl NonceManager {
    /// Returns the next available nonce and increments the in-flight counter.
    pub async fn next_nonce(&mut self) -> u64 {
        let nonce = self.base_nonce + self.in_flight;
        self.in_flight += 1;
        nonce
    }

    /// Marks a transaction as completed by decrementing the in-flight counter.
    pub async fn complete(&mut self) {
        if self.in_flight > 0 {
            self.in_flight -= 1;
        }
    }
}

/// Default number of results per page for queries
pub const DEFAULT_RESULTS_PER_PAGE: u64 = 100;

/// Configuration for transaction parameters.
/// Holds gas and fee settings used for sending transactions.
#[derive(Debug, Clone)]
pub struct TransactionConfig {
    /// Gas limit for transactions.
    pub gas_limit: u64,
    /// Maximum priority fee per gas (in wei).
    pub max_priority_fee_per_gas: u128,
    /// Maximum fee per gas (in wei).
    pub max_fee_per_gas: u128,
    /// Timeout for waiting for transaction receipts.
    pub transaction_receipt_timeout: Duration,
    /// Timeout for waiting for pending transactions to be mined.
    pub pending_transaction_timeout: Duration,
    /// Maximum number of retries for transaction receipt polling.
    pub max_retries: u32,
    /// Percentage bump for replacement transactions (e.g. 10 for 10%).
    pub price_bump_percent: u128,
    /// Number of confirmations to wait for when watching pending transactions.
    pub required_confirmations: u64,
    /// Optional chain ID for validation. If None, SDK will query chain ID from chain.
    pub chain_id: Option<u64>,
    /// Default number of results per page for queries.
    pub default_results_per_page: u64,
    /// Maximum number of query errors to ignore before giving up.
    pub max_query_errors: u32,
}

impl Default for TransactionConfig {
    fn default() -> Self {
        Self {
            gas_limit: 1_000_000,
            max_priority_fee_per_gas: 1,
            max_fee_per_gas: 2_000_000,
            transaction_receipt_timeout: Duration::from_secs(60),
            pending_transaction_timeout: Duration::from_secs(10),
            max_retries: 3,
            price_bump_percent: 100,
            required_confirmations: 0,
            chain_id: None,
            default_results_per_page: DEFAULT_RESULTS_PER_PAGE,
            max_query_errors: 3,
        }
    }
}

/// A client for interacting with the Arkiv system.
/// Provides methods for account management, entity operations, balance queries, and event subscriptions.
#[derive(Clone)]
pub struct ArkivClient {
    /// The underlying provider for making RPC calls.
    pub(crate) provider: ResilientProvider,
    /// Registered accounts mapped by address.
    pub(crate) accounts: Arc<RwLock<HashMap<Address, Account>>>,
    /// The URL of the Arkiv endpoint.
    pub(crate) rpc_url: Url,
    /// The Ethereum address of the client owner.
    pub(crate) wallet: PrivateKeySigner,
    /// Transaction configuration.
    pub(crate) tx_config: Arc<TransactionConfig>,
    /// Nonce manager for tracking transaction nonces.
    pub(crate) nonce_manager: Arc<Mutex<NonceManager>>,
}

#[bon]
impl ArkivClient {
    /// Creates a new builder for `ArkivClient` with the given wallet and RPC URL.
    /// Initializes the provider and sets up default configuration.
    #[builder]
    pub fn builder(wallet: PrivateKeySigner, rpc_url: Url) -> Self {
        let provider = ProviderBuilder::new()
            .fetch_chain_id()
            .connect_http(rpc_url.clone())
            .erased();

        Self {
            provider: ResilientProvider::from(provider),
            accounts: Arc::new(RwLock::new(HashMap::new())),
            rpc_url,
            wallet,
            tx_config: Arc::new(TransactionConfig::default()),
            nonce_manager: Arc::new(Mutex::new(NonceManager {
                base_nonce: 0,
                in_flight: 0,
            })),
        }
    }

    /// Gets the underlying Reqwest client used for HTTP requests.
    pub fn get_reqwest_client(&self) -> ClientRef<'_> {
        self.provider.inner().client()
    }

    /// Gets the underlying RPC client (provider) used for blockchain interactions.
    pub fn get_rpc_client(&self) -> ResilientProvider {
        self.provider.clone()
    }

    /// Gets the Ethereum address of the client owner.
    pub fn get_owner_address(&self) -> Address {
        self.wallet.address()
    }

    /// Creates a new client with the given endpoint.
    /// Initializes with a random wallet.
    pub fn new(endpoint: Url) -> anyhow::Result<Self> {
        Self::new_uninitialized(endpoint)
    }

    /// Creates a new client without initializing it.
    /// Useful for advanced scenarios or custom initialization.
    pub fn new_uninitialized(endpoint: Url) -> anyhow::Result<Self> {
        let provider = ProviderBuilder::new()
            .connect_http(endpoint.clone())
            .erased();

        Ok(Self {
            provider: ResilientProvider::from(provider),
            accounts: Arc::new(RwLock::new(HashMap::new())),
            rpc_url: endpoint,
            wallet: PrivateKeySigner::random(),
            tx_config: Arc::new(TransactionConfig::default()),
            nonce_manager: Arc::new(Mutex::new(NonceManager {
                base_nonce: 0,
                in_flight: 0,
            })),
        })
    }

    pub fn override_config(mut self, tx_config: TransactionConfig) -> Self {
        self.tx_config = Arc::new(tx_config);
        self
    }

    /// Gets the chain ID from the provider.
    /// Returns the chain ID as a `u64`.
    pub async fn get_chain_id(&self) -> anyhow::Result<u64> {
        self.provider
            .get_chain_id()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get chain ID: {}", e))
    }

    /// Validates the configured chain ID against the actual chain ID.
    /// If no chain ID is configured in TransactionConfig, this method does nothing.
    /// Returns the actual chain ID and an error if the configured chain ID doesn't match.
    pub async fn validate_chain_id(&self) -> anyhow::Result<u64> {
        let actual = self.get_chain_id().await?;
        if let Some(configured) = self.tx_config.chain_id {
            if configured != actual {
                return Err(anyhow::anyhow!(
                    "Chain ID mismatch: configured {configured} but actual chain ID is {actual}"
                ));
            }
        }
        Ok(actual)
    }

    /// Checks chain ID and syncs accounts with the GolemBase node.
    /// Waits until the node is synced or the timeout is reached.
    pub async fn sync_node(&self, timeout: Duration) -> anyhow::Result<()> {
        let start_time = Instant::now();
        let stop_time = start_time + timeout;

        while !self.is_synced().await? {
            if Instant::now() > stop_time {
                return Err(anyhow::anyhow!(
                    "Timeout {} while syncing node",
                    humantime::format_duration(timeout)
                ));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // Validate chain ID if configured and get the actual chain ID
        let chain_id = self.validate_chain_id().await?;
        self.sync_arkiv_accounts(chain_id).await?;
        Ok(())
    }

    /// Registers a user-managed account with a custom signer.
    /// Returns the registered account's address.
    pub async fn account_register(
        &self,
        signer: impl TransactionSigner + 'static,
    ) -> anyhow::Result<Address> {
        // Validate chain ID if configured and get the actual chain ID
        let chain_id = self.validate_chain_id().await?;
        let address = signer.address();
        let mut accounts = self.accounts.write().unwrap();
        accounts.insert(
            address,
            Account::new(
                Box::new(signer),
                self.provider.clone(),
                chain_id,
                self.tx_config.clone(),
            ),
        );

        Ok(address)
    }

    /// Generates a new local key, saves it to a keystore file, and registers it.
    /// Returns the address of the new account.
    pub async fn account_generate(&self, password: &str) -> anyhow::Result<Address> {
        let signer = InMemorySigner::generate();
        let _path = signer
            .save(password)
            .map_err(|e| anyhow::anyhow!("Failed to save account: {e}"))?;
        self.account_register(signer).await
    }

    /// Loads a key from a raw private key file or keystore and registers it.
    /// Returns the address of the loaded account.
    pub async fn account_load_file(
        &self,
        path: PathBuf,
        password: &str,
    ) -> anyhow::Result<Address> {
        // First try to load as keystore.
        let signer = match InMemorySigner::load_keystore(path.clone(), password) {
            Ok(signer) => signer,
            Err(_) => {
                // If keystore loading fails, try as raw key file
                InMemorySigner::load_raw_key(path)?
            }
        };
        self.account_register(signer).await
    }

    /// Loads a key from the default directory and registers it.
    /// Returns the address if successful.
    pub async fn account_load(&self, address: Address, password: &str) -> anyhow::Result<Address> {
        // This will load all available accounts from GolemBase.
        // We check only the registered accounts, because sync returns local as well.
        let all_accounts = self
            .account_sync()
            .await
            .map_err(|e| anyhow::anyhow!("Sync-ing accounts: {e}"))?;
        if self.accounts_list().contains(&address) {
            return Ok(address);
        }

        if !all_accounts.contains(&address) {
            return Err(anyhow::anyhow!(
                "Account {address} not found in available accounts"
            ));
        }

        // Try to load from local keystore if it wasn't loaded from GolemBase.
        let signer = InMemorySigner::load_by_address(address, password)?;
        self.account_register(signer).await
    }

    /// Lists all registered accounts.
    /// Returns a vector of `Address`.
    pub fn accounts_list(&self) -> Vec<Address> {
        let accounts = self.accounts.read().unwrap();
        accounts.keys().cloned().collect()
    }

    /// Synchronizes accounts with GolemBase, adding any new accounts to local state.
    /// Returns a vector of all available account addresses.
    pub async fn account_sync(&self) -> anyhow::Result<Vec<Address>> {
        // Validate chain ID if configured and get the actual chain ID
        let chain_id = self.validate_chain_id().await?;

        // Sync Arkiv accounts
        self.sync_arkiv_accounts(chain_id).await?;

        // Get all available accounts
        let mut all_accounts = self.accounts_list();
        let local_accounts = InMemorySigner::list_local_accounts()?;

        // Add local accounts that aren't already in the list
        for address in local_accounts {
            if !all_accounts.contains(&address) {
                all_accounts.push(address);
            }
        }

        Ok(all_accounts)
    }

    /// Gets an account's ETH balance as a `BigDecimal`.
    pub async fn get_balance(&self, account: Address) -> anyhow::Result<BigDecimal> {
        let balance = self.provider.get_balance(account).await?;
        Ok(wei_to_eth(balance))
    }

    /// Transfers ETH from one account to another.
    /// Returns the transaction hash.
    pub async fn transfer(
        &self,
        from: Address,
        to: Address,
        value: BigDecimal,
    ) -> anyhow::Result<B256> {
        let account = self.account_get(from)?;
        let receipt = account.transfer(to, value).await?;
        Ok(receipt.transaction_hash)
    }

    /// Funds an account with ETH.
    /// Returns the transaction hash.
    pub async fn fund(&self, account: Address, value: BigDecimal) -> anyhow::Result<B256> {
        let account = self.account_get(account)?;
        let receipt = account.fund_account(value).await?;
        Ok(receipt.transaction_hash)
    }

    /// Internal: synchronizes Arkiv accounts with the current chain ID.
    async fn sync_arkiv_accounts(&self, chain_id: u64) -> anyhow::Result<()> {
        let arkiv_accounts = self.list_arkiv_accounts().await?;
        let mut accounts = self.accounts.write().unwrap();

        for address in arkiv_accounts {
            self.try_insert_account(&mut accounts, address, chain_id, |address| {
                Box::new(ArkivSigner::new(address, self.provider.clone(), chain_id))
            });
        }

        Ok(())
    }

    /// Internal: inserts an account if not already present.
    fn try_insert_account<F>(
        &self,
        accounts: &mut HashMap<Address, Account>,
        address: Address,
        chain_id: u64,
        create_signer: F,
    ) where
        F: FnOnce(Address) -> Box<dyn TransactionSigner>,
    {
        if accounts.contains_key(&address) {
            return;
        }

        let signer = create_signer(address);
        accounts.insert(
            address,
            Account::new(
                signer,
                self.provider.clone(),
                chain_id,
                self.tx_config.clone(),
            ),
        );
    }

    /// Gets an account by its address.
    /// Returns an `Account` if found, or an error otherwise.
    pub fn account_get(&self, address: Address) -> anyhow::Result<Account> {
        let accounts = self.accounts.read().unwrap();
        accounts
            .get(&address)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Account {address} not found"))
    }

    /// Internal: lists accounts from Arkiv.
    async fn list_arkiv_accounts(&self) -> anyhow::Result<Vec<Address>> {
        Ok(self.provider.get_accounts().await?)
    }

    /// Creates an entry using the specified account.
    /// Returns the entity ID of the created entry.
    pub async fn create_entry(&self, account: Address, entry: Create) -> anyhow::Result<Hash> {
        let account = self.account_get(account)?;
        let tx = ArkivTransaction {
            creates: vec![entry],
            updates: vec![],
            deletes: vec![],
            extensions: vec![],
        };

        log::debug!("Sending storage transaction from {}", account.address());

        let receipt = account.send_db_transaction(tx).await?;
        if !receipt.status() {
            return Err(anyhow::anyhow!(
                "Transaction {} failed despite being mined.",
                receipt.transaction_hash
            ));
        }

        // Parse logs to get entity ID
        let entity_id = Self::extract_entity_id(receipt.logs())?;

        log::debug!("Created entity with ID: 0x{:x}", entity_id);
        Ok(entity_id)
    }

    /// Extracts entity ID from transaction logs by looking for ArkivStorageEntityCreated events.
    /// Returns the entity ID if found, or an error if not found.
    pub fn extract_entity_id(logs: &[Log]) -> anyhow::Result<Hash> {
        logs.iter()
            .inspect(|log| log::trace!("Log: {:?}", log))
            .find_map(|log| {
                if log.topics().len() >= 2 && log.topics()[0] == arkiv_storage_entity_created() {
                    // Second topic is the entity ID
                    Some(log.topics()[1])
                } else {
                    None
                }
            })
            .ok_or_else(|| anyhow::anyhow!("No entity ID found in transaction logs"))
    }

    /// Removes entries from Arkiv.
    /// Deletes the specified entries owned by the given account.
    ///
    /// # Arguments
    /// * `account` - The account address that owns the entries.
    /// * `entry_ids` - The IDs of the entries to remove.
    pub async fn remove_entries(
        &self,
        account: Address,
        entry_ids: Vec<Hash>,
    ) -> anyhow::Result<()> {
        if entry_ids.is_empty() {
            return Ok(());
        }

        let account = self.account_get(account)?;
        let entry_count = entry_ids.len();
        let tx = ArkivTransaction {
            creates: vec![],
            updates: vec![],
            deletes: entry_ids,
            extensions: vec![],
        };

        log::debug!(
            "Sending delete transaction from {} for {} entries",
            account.address(),
            entry_count
        );

        let receipt = account.send_db_transaction(tx).await?;
        if !receipt.status() {
            return Err(anyhow::anyhow!(
                "Transaction {} failed despite being mined.",
                receipt.transaction_hash
            ));
        }

        log::debug!("Successfully removed {} entries", entry_count);
        Ok(())
    }

    /// Retrieves an entry's payload from Arkiv by its ID.
    /// Returns the entry data as a `String`.
    pub async fn cat(&self, id: Hash) -> anyhow::Result<String> {
        let bytes = self.get_storage_value::<Bytes>(id).await?;
        Ok(String::from_utf8(bytes.to_vec()).map_err(|e| Error::UnexpectedError(e.to_string()))?)
    }

    /// Checks if the node is synced by comparing the latest block timestamp with current time.
    /// Returns `true` if the node is synced (latest block is less than 5 minutes old).
    pub async fn is_synced(&self) -> anyhow::Result<bool> {
        let syncing = self.provider.syncing().await?;
        match syncing {
            SyncStatus::Info(sync) => {
                let current_block = sync.current_block;
                let highest_block = sync.highest_block;

                if current_block == highest_block {
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            SyncStatus::None => Ok(true),
        }
    }

    /// Updates an entry using the specified account.
    /// Sends an update transaction for the given entry.
    ///
    /// # Arguments
    /// * `account` - The account address that owns the entry.
    /// * `update` - The update operation containing new data and annotations.
    pub async fn update_entry(&self, account: Address, update: Update) -> anyhow::Result<()> {
        let entity_key = update.entity_key;
        let account = self.account_get(account)?;
        let tx = ArkivTransaction {
            creates: vec![],
            updates: vec![update],
            deletes: vec![],
            extensions: vec![],
        };

        log::debug!(
            "Sending update transaction from {} for entry 0x{:x}",
            account.address(),
            entity_key
        );

        let receipt = account.send_db_transaction(tx).await?;
        if !receipt.status() {
            return Err(anyhow::anyhow!(
                "Transaction {} failed despite being mined.",
                receipt.transaction_hash
            ));
        }

        log::debug!("Successfully updated entry with ID: 0x{:x}", entity_key);
        Ok(())
    }

    /// Gets the current block number from the chain.
    /// Returns the latest block number as a `u64`.
    pub async fn get_current_block_number(&self) -> anyhow::Result<u64> {
        let latest_block = self
            .provider
            .get_block_by_number(BlockNumberOrTag::Latest)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Failed to get latest block"))?;
        Ok(latest_block.header.number)
    }

    /// Waits for a arbitrary (not created by this client) transaction to be mined and returns
    /// its receipt. Handles retries for transaction indexing error.
    pub async fn wait_for_transaction(&self, tx_hash: Hash) -> anyhow::Result<TransactionReceipt> {
        crate::account::get_receipt(
            &self.provider,
            tx_hash,
            None,
            self.tx_config.required_confirmations,
        )
        .await?
        .ok_or_else(|| anyhow::anyhow!("Transaction receipt not found for hash: {}", tx_hash))
    }

    /// Creates a new WebSocket client for event subscriptions using the default RPC URL.
    pub async fn events_client(&self) -> anyhow::Result<EventsClient> {
        let mut ws_url = self.rpc_url.clone();
        ws_url.set_scheme("ws").unwrap();
        EventsClient::new(ws_url).await
    }

    /// Creates a new WebSocket client for event subscriptions with a custom WebSocket URL.
    pub async fn events_client_with_url(&self, ws_url: Url) -> anyhow::Result<EventsClient> {
        EventsClient::new(ws_url).await
    }
}
