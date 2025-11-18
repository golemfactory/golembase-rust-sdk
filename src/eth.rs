use crate::entity::Hash;
use crate::entity::{
    ArkivTransaction, Create, DeleteResult, EntityResult, Extend, ExtendResult, Update,
};
use crate::ArkivClient;

use alloy::primitives::{address, Address, TxKind};
use alloy::providers::Provider;
use alloy::providers::ProviderBuilder;
use alloy::rpc::types::{Log, TransactionReceipt, TransactionRequest};
use alloy_rlp::Encodable;
use displaydoc::Display;
use thiserror::Error;

/// Represents errors that can occur in the Arkiv ETH client.
/// Used for wrapping transaction, receipt, and log decoding errors.
#[derive(Debug, Display, Error)]
pub enum Error {
    /// Failed to send transaction: {0}
    TransactionSendError(String),
    /// Failed to get transaction receipt: {0}
    TransactionReceiptError(String),
    /// Failed to decode expiration block: {0}
    ExpirationBlockDecodeError(String),
    /// Unexpected log data format
    UnexpectedLogDataError,
}

/// The Ethereum address of the Arkiv storage contract.
/// All entity-related transactions are sent to this address.
pub const STORAGE_ADDRESS: Address = address!("0x00000000000000000000000000000061726B6976");

impl ArkivClient {
    /// Creates one or more new entities in Arkiv and returns their results.
    /// Sends a transaction to the storage contract and parses the resulting logs.
    pub async fn create_entities(&self, creates: Vec<Create>) -> Result<Vec<EntityResult>, Error> {
        let receipt = self
            .create_raw_transaction(ArkivTransaction {
                creates,
                updates: vec![],
                deletes: vec![],
                extensions: vec![],
                change_owners: vec![],
            })
            .await?;
        self.process_receipt(receipt, |log| {
            if log.topics().len() < 2 {
                return None;
            }
            let expiration_block =
                Self::parse_expiration_block(log.data().data.as_ref(), /*word_index=*/ 0);
            Some(EntityResult {
                entity_key: log.topics()[1],
                expiration_block,
            })
        })
        .await
    }

    /// Updates one or more entities in Arkiv and returns their results.
    /// Sends a transaction to the storage contract and parses the resulting logs.
    pub async fn update_entities(&self, updates: Vec<Update>) -> Result<Vec<EntityResult>, Error> {
        let receipt = self
            .create_raw_transaction(ArkivTransaction {
                creates: vec![],
                updates,
                deletes: vec![],
                extensions: vec![],
                change_owners: vec![],
            })
            .await?;
        self.process_receipt(receipt, |log| {
            if log.topics().len() < 2 {
                return None;
            }
            let expiration_block =
                Self::parse_expiration_block(log.data().data.as_ref(), /*word_index=*/ 1);
            Some(EntityResult {
                entity_key: log.topics()[1],
                expiration_block,
            })
        })
        .await
    }

    /// Deletes one or more entities in Arkiv and returns their results.
    /// Sends a transaction to the storage contract and parses the resulting logs.
    pub async fn delete_entities(&self, deletes: Vec<Hash>) -> Result<Vec<DeleteResult>, Error> {
        let receipt = self
            .create_raw_transaction(ArkivTransaction {
                creates: vec![],
                updates: vec![],
                deletes,
                extensions: vec![],
                change_owners: vec![],
            })
            .await?;
        self.process_receipt(receipt, |log| {
            if log.topics().len() < 2 {
                return None;
            }
            Some(DeleteResult {
                entity_key: log.topics()[1],
            })
        })
        .await
    }

    /// Extends the BTL (block time to live) of one or more entities and returns their results.
    /// Sends a transaction to the storage contract and parses the resulting logs for old and new expiration blocks.
    pub async fn extend_entities(
        &self,
        extensions: Vec<Extend>,
    ) -> Result<Vec<ExtendResult>, Error> {
        let receipt = self
            .create_raw_transaction(ArkivTransaction {
                creates: vec![],
                updates: vec![],
                deletes: vec![],
                extensions,
                change_owners: vec![],
            })
            .await?;
        self.process_receipt(receipt, |log| {
            let data = log.data().data.as_ref();
            if log.topics().len() < 2 {
                return None;
            }
            let old_expiration_block = Self::parse_expiration_block(data, /*word_index=*/ 0);
            let new_expiration_block = Self::parse_expiration_block(data, /*word_index=*/ 1);
            Some(ExtendResult {
                entity_key: log.topics()[1],
                old_expiration_block,
                new_expiration_block,
            })
        })
        .await
    }

    /// Creates and sends a raw transaction to the Arkiv storage contract.
    /// Encodes the transaction payload and sends it to the contract address.
    pub async fn create_raw_transaction(
        &self,
        payload: ArkivTransaction,
    ) -> Result<TransactionReceipt, Error> {
        log::debug!("payload: {:?}", payload);
        let mut buffer = Vec::new();
        payload.encode(&mut buffer);
        log::debug!("buffer: {:?}", buffer);
        let nonce = {
            let mut nm = self.nonce_manager.lock().await;
            if nm.in_flight == 0 {
                let wallet_address = self.wallet.address();
                nm.base_nonce = self
                    .provider
                    .get_transaction_count(wallet_address)
                    .await
                    .map_err(|e| Error::TransactionSendError(e.to_string()))?;
            }
            nm.next_nonce().await
        };
        let tx = TransactionRequest {
            to: Some(TxKind::Call(STORAGE_ADDRESS)),
            input: buffer.into(),
            chain_id: Some(
                self.provider
                    .get_chain_id()
                    .await
                    .map_err(|e| Error::TransactionSendError(e.to_string()))?,
            ),
            nonce: Some(nonce),
            ..Default::default()
        };
        log::debug!("transaction: {:?}", tx);
        let provider = ProviderBuilder::new()
            .wallet(self.wallet.clone())
            .connect_http(self.rpc_url.clone());
        log::debug!("provider: {:?}", provider);
        let pending_tx = provider
            .send_transaction(tx)
            .await
            .map_err(|e| Error::TransactionSendError(e.to_string()))?;
        log::debug!("pending transaction: {:?}", pending_tx);
        let receipt = pending_tx
            .get_receipt()
            .await
            .map_err(|e| Error::TransactionReceiptError(e.to_string()))?;
        log::debug!("receipt: {:?}", receipt);
        {
            let mut nm = self.nonce_manager.lock().await;
            nm.complete().await;
        }
        Ok(receipt)
    }

    /// Processes a transaction receipt and maps logs into the desired result type.
    /// Filters logs for the storage contract and applies the provided mapping function.
    async fn process_receipt<T, F>(
        &self,
        receipt: TransactionReceipt,
        log_mapper: F,
    ) -> Result<Vec<T>, Error>
    where
        F: Fn(&Log) -> Option<T>,
    {
        let results: Vec<T> = receipt
            .logs()
            .iter()
            .filter(|log| log.address() == STORAGE_ADDRESS)
            .filter_map(log_mapper)
            .collect();
        Ok(results)
    }

    /// Parses a single `u64` value from log data, padding the beginning with zeros if needed.
    /// Used to extract expiration block numbers from log data fields.
    fn parse_expiration_block(data: &[u8], word_index: usize) -> u64 {
        let start = word_index.saturating_mul(32);
        if data.len() < start + 32 {
            return 0;
        }
        let word = &data[start..start + 32];
        let mut padded = [0u8; 8];
        padded.copy_from_slice(&word[24..32]);
        u64::from_be_bytes(padded)
    }
}
