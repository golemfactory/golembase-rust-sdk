use alloy::primitives::Address;
use alloy::signers::local::PrivateKeySigner;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Manages accounts with their private key signers
#[derive(Clone, Debug)]
pub struct ManagedAccounts {
    accounts: Arc<Mutex<HashMap<Address, PrivateKeySigner>>>,
    internal_account: PrivateKeySigner,
}

impl Default for ManagedAccounts {
    fn default() -> Self {
        Self::new()
    }
}

impl ManagedAccounts {
    pub fn new() -> Self {
        Self {
            accounts: Arc::new(Mutex::new(HashMap::new())),
            internal_account: PrivateKeySigner::random(),
        }
    }

    /// Creates a new account with a random private key
    pub fn create_account(&self) -> Address {
        let mut accounts = self.accounts.lock().unwrap();
        let signer = PrivateKeySigner::random();
        let address = signer.address();
        accounts.insert(address, signer);
        address
    }

    /// Gets an account for the given address, returns None if it doesn't exist
    pub fn get_account(&self, address: Address) -> Option<PrivateKeySigner> {
        let accounts = self.accounts.lock().unwrap();
        accounts.get(&address).cloned()
    }

    /// Gets all managed account addresses
    pub fn get_all_accounts(&self) -> Vec<Address> {
        let accounts = self.accounts.lock().unwrap();
        accounts.keys().cloned().collect()
    }

    /// Gets the internal account for housekeeping transactions
    /// This account is not accessible to outside users
    pub fn get_internal_account(&self) -> PrivateKeySigner {
        self.internal_account.clone()
    }
}
