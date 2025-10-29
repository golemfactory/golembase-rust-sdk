//! Arkiv container testing utilities.
//!
//! This module provides utilities for running Arkiv in containers for testing purposes.

use std::time::Duration;
use tempfile::TempDir;
use testcontainers::core::logs::LogFrame;
use testcontainers::core::{ContainerPort, ContainerRequest, Mount, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};
use url::Url;

/// Configuration for Arkiv container.
pub struct Config {
    /// Port for the Arkiv instance
    pub port: u16,
    /// Timeout for waiting for container to start
    pub timeout: Duration,
    /// Container image to use
    pub image: String,
    /// Container tag to use
    pub tag: String,
    /// Temporary directory for volume preservation (None if not preserving volumes)
    pub data_dir: Option<TempDir>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: 9545,
            timeout: Duration::from_secs(120), // Increased timeout for stability
            image: "golemnetwork/golembase-op-geth".to_string(),
            tag: "latest".to_string(),
            data_dir: None,
        }
    }
}

impl Config {
    /// Set the port for the Arkiv instance
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Set the timeout for container operations
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Enable volume preservation between restarts.
    /// This will generate a unique temporary directory and mount it to preserve:
    /// - geth_data: Ethereum node data
    /// - arkiv_wal: Arkiv write-ahead log
    /// - config: Arkiv configuration
    pub fn preserve_volume(mut self) -> Self {
        self.data_dir = Some(Self::generate_temp_data_dir());
        self
    }

    /// Generate a unique temporary directory for Arkiv data.
    fn generate_temp_data_dir() -> TempDir {
        tempfile::Builder::new()
            .prefix("arkiv-data-")
            .tempdir()
            .expect("Failed to create temporary directory")
    }

    /// Apply volume mounts to a container request if volume preservation is enabled.
    pub fn apply_volume_mounts(
        &self,
        mut container_request: ContainerRequest<GenericImage>,
    ) -> anyhow::Result<ContainerRequest<GenericImage>> {
        if let Some(data_dir) = &self.data_dir {
            let dir = data_dir.path();
            let geth_data = dir.join("geth_data");
            let config = dir.join("config");

            log::info!("Mounting data directory: {}", dir.display());

            std::fs::create_dir_all(&geth_data)?;
            std::fs::create_dir_all(&config)?;

            // Mount geth_data directory as named volume
            let geth_data_mount = Mount::bind_mount(geth_data.display().to_string(), "/geth_data");
            container_request = container_request.with_mount(geth_data_mount);

            // Mount arkiv config directory as named volume
            let config_mount =
                Mount::bind_mount(config.display().to_string(), "/root/.config/arkiv");
            container_request = container_request.with_mount(config_mount);
        }
        Ok(container_request)
    }
}

/// Wrapper for Arkiv container that provides helper functions.
pub struct ArkivContainer {
    container: ContainerAsync<GenericImage>,
    config: Config,
    mapped_port: u16,
}

impl ArkivContainer {
    /// Initialize a new Arkiv container with the given configuration.
    pub async fn new(config: Config) -> Result<Self, anyhow::Error> {
        let container = Self::init_arkiv(&config).await?;
        let mapped_port = container.get_host_port_ipv4(config.port).await?;
        Ok(Self {
            container,
            config,
            mapped_port,
        })
    }

    /// Get the container URL that can be used with ArkivClient.
    pub fn get_url(&self) -> Result<Url, anyhow::Error> {
        Ok(Url::parse(&format!(
            "http://localhost:{}",
            self.mapped_port
        ))?)
    }

    /// Get the container ID for debugging purposes.
    pub fn container_id(&self) -> String {
        self.container.id().to_string()
    }

    /// Get a reference to the configuration.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Stop the container.
    /// This stops all processes in the container.
    pub async fn stop(&self) -> Result<(), anyhow::Error> {
        Ok(self.container.stop().await?)
    }

    /// Restart the container with the same configuration.
    /// This stops the current container and starts a new one.
    pub async fn restart(&mut self) -> Result<(), anyhow::Error> {
        // Stop the current container
        self.stop().await?;

        // Initialize a new container with the same configuration
        let new_container = Self::init_arkiv(&self.config).await?;
        let new_mapped_port = new_container.get_host_port_ipv4(self.config.port).await?;

        // Update the container and mapped port
        self.container = new_container;
        self.mapped_port = new_mapped_port;

        Ok(())
    }

    /// Pause the container.
    /// This suspends all processes in the container.
    pub async fn pause(&self) -> Result<(), anyhow::Error> {
        Ok(self.container.pause().await?)
    }

    /// Unpause the container.
    /// This resumes all processes in the container.
    pub async fn unpause(&self) -> Result<(), anyhow::Error> {
        Ok(self.container.unpause().await?)
    }

    /// Initialize the Arkiv container with the given configuration.
    async fn init_arkiv(config: &Config) -> Result<ContainerAsync<GenericImage>, anyhow::Error> {
        let port = config.port;
        let timeout = config.timeout;

        let mut container_request = GenericImage::new(&config.image, &config.tag)
            .with_wait_for(WaitFor::message_on_stderr("HTTP server started"))
            //.with_mapped_port(port, ContainerPort::Tcp(port))
            .with_exposed_port(ContainerPort::Tcp(port))
            .with_log_consumer(|line: &LogFrame| {
                log::info!("[Arkiv]: {}", String::from_utf8_lossy(&line.bytes()))
            })
            .with_cmd([
                "--dev",
                "--http",
                "--http.api",
                "eth,web3,net,debug,golembas,arkiv",
                "--verbosity",
                "3",
                "--http.addr",
                "0.0.0.0",
                "--http.port",
                &port.to_string(),
                "--http.corsdomain",
                "*",
                "--http.vhosts",
                "*",
                "--ws",
                "--ws.addr",
                "0.0.0.0",
                "--ws.port",
                &port.to_string(),
                "--datadir",
                "/geth_data",
            ])
            .with_env_var("GITHUB_ACTIONS", "true")
            .with_env_var("CI", "true");

        // Apply volume mounts if volume preservation is enabled
        container_request = config.apply_volume_mounts(container_request)?;

        let container_future = container_request.start();

        let container = match tokio::time::timeout(timeout, container_future).await {
            Ok(Ok(container)) => container,
            Ok(Err(e)) => return Err(anyhow::anyhow!("Failed to start Arkiv instance: {}", e)),
            Err(_) => {
                return Err(anyhow::anyhow!(
                    "Timeout ({}) starting Arkiv instance",
                    humantime::format_duration(timeout)
                ))
            }
        };

        Ok(container)
    }
}
