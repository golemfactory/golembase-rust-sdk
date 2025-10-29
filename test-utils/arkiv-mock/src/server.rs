use alloy::primitives::{Address, U256};
use jsonrpsee::server::{RpcModule, ServerBuilder};
use std::net::SocketAddr;
use url::Url;

use crate::api::{ArkivRpcServer, EthRpcServer};
use crate::controller::MockController;
use crate::transaction_pool::TransactionPool;
use crate::ArkivMock;

/// Arkiv Mock Server
#[derive(Clone)]
pub struct ArkivMockServer {
    pub state: ArkivMock,
    pub url: Url,
    #[allow(dead_code)]
    server: Option<jsonrpsee::server::ServerHandle>,
}

impl ArkivMockServer {
    pub fn new() -> Self {
        Self {
            state: ArkivMock::new(),
            url: Url::parse("http://127.0.0.1:8585").unwrap(),
            server: None,
        }
    }

    pub fn controller(&self) -> &MockController {
        &self.state.controller
    }

    pub fn transaction_pool(&self) -> &TransactionPool {
        &self.state.transaction_pool
    }

    pub fn with_chain_id(self, chain_id: u64) -> Self {
        self.state.set_chain_id(chain_id);
        self
    }

    pub fn with_url(mut self, url: Url) -> Self {
        self.url = url;
        self
    }

    pub async fn create_test_account(&mut self, initial_balance: U256) -> Address {
        let address = self.state.create_account();
        self.state.blockchain.add_accounts(vec![address]).await;
        self.state
            .blockchain
            .set_balance(address, initial_balance)
            .await;
        address
    }

    pub async fn start(self, addr: SocketAddr) -> anyhow::Result<Self> {
        let mut module = RpcModule::new(());

        // Register RPC methods (both Ethereum and GolemBase)
        let rpc_impl = self.state.clone();
        module.merge(EthRpcServer::into_rpc(rpc_impl.clone()))?;
        module.merge(ArkivRpcServer::into_rpc(rpc_impl))?;

        let server = ServerBuilder::default().build(addr).await?;

        let actual_addr = server.local_addr()?;
        log::info!("GolemBase Mock Server listening on {}", actual_addr);

        // Start the execution engine to produce blocks
        self.state.blockchain.create_genesis_block().await;
        self.state.execution.start().await;

        let server_handle = server.start(module);

        Ok(Self {
            state: self.state,
            url: self.url,
            server: Some(server_handle),
        })
    }

    /// Get the server URL
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// Resolve host name to IP address for localhost
    fn resolve_host(host: &str) -> String {
        match host {
            "localhost" => "127.0.0.1".to_string(),
            _ => host.to_string(),
        }
    }

    /// Convert URL to SocketAddr
    pub fn socket_addr(&self) -> anyhow::Result<SocketAddr> {
        let url = self.url.clone();
        let host = url
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("URL has no host"))?;
        let port = url
            .port()
            .ok_or_else(|| anyhow::anyhow!("URL has no port specified"))?;

        let resolved_host = Self::resolve_host(host);
        let addr = format!("{}:{}", resolved_host, port);
        addr.parse()
            .map_err(|e| anyhow::anyhow!("Failed to parse socket address: {}", e))
    }

    /// Create a test mock server with test accounts and balances
    pub async fn create_test_mock_server() -> anyhow::Result<Self> {
        let server = Self::new().with_chain_id(1337);
        server.default_start().await
    }

    pub async fn default_start(mut self) -> anyhow::Result<Self> {
        // Create test accounts with initial balance, to fund other accounts
        self.create_test_account(U256::from(1000000000000000000000u128))
            .await;

        // Start the server
        let socket_addr = self.socket_addr()?;
        let server = self.start(socket_addr).await?;
        Ok(server)
    }
}
