// # Arkiv SDK
//!
//! This is part of the [Arkiv](https://github.com/Arkiv) project, which is designed as a Layer2 Network deployed on Ethereum, acting as a gateway to various Layer 3 Database Chains (DB-Chains).
//! For an overview of Arkiv, **check out our [Litepaper](https://arkiv.io/wp-content/uploads/2025/03/Arkiv-Litepaper.pdf)**.
//!
//! This SDK allows you to use [Arkiv](https://github.com/Arkiv) from Rust, it is available on [crates.io](https://crates.io/crates/arkiv-sdk), along with its [generated documentation](https://docs.rs/arkiv-sdk). We provide an [example application](https://github.com/Arkiv/rust-sdk/tree/main/demo) to showcase how you can use this SDK.
//!
//! For **getting up and running quickly**, we recommend the following two steps:
//! 1. Start arkiv-op-geth through its [`docker-compose`](https://github.com/Arkiv/arkiv-op-geth/blob/main/RUN_LOCALLY.md) ;
//! 2. [Install the demo CLI](https://github.com/Arkiv/arkiv-demo-cli?tab=readme-ov-file#installation) and [create a user](https://github.com/Arkiv/arkiv-demo-cli?tab=readme-ov-file#quickstart), or build the [actual CLI](https://github.com/Arkiv/arkiv-op-geth/blob/main/cmd/arkiv/README.md) as it's included in the `arkiv-op-geth` repository.
//!
//! When you create a user, it will generate a private key file called `private.key` and store it in the standard folder as per the [XDG specification](https://specifications.freedesktop.org/basedir-spec/latest/):
//! - `~/.config/arkiv/` on **Linux**
//! - `~/Library/Application Support/arkiv/` on **macOS**
//! - `%LOCALAPPDATA%\arkiv\` on **Windows**
//!
//! You will also need to fund the account, you can do it with: `arkiv-demo-cli account fund 10`
//!
//! # Transaction Abstractions
//!
//! This SDK provides multiple layers for sending transactions:
//! - Use [`ArkivClient`] for high-level operations such as creating, updating, or deleting entities.
//! - Use [`Account`](crate::account::Account) for account-centric and lower-level transaction control.
//! - Advanced users can construct and submit raw Ethereum transactions directly using the types and helpers re-exported from `Alloy`.

/// Re-export commonly used types from `alloy`.
pub use alloy::primitives::{keccak256, Address};
pub use alloy::signers::local::PrivateKeySigner;
pub use alloy::signers::Signature;
pub use alloy::transports::http::reqwest::Url;

pub use client::ArkivClient;
pub use entity::{Annotation, Hash, NumericAnnotation, StringAnnotation};

/// Module for Ethereum transaction-related functionality.
/// Provides helpers for constructing, signing, and sending Ethereum transactions.
pub mod eth;

/// Module for JSON-RPC-related functionality.
/// Contains utilities for interacting with JSON-RPC endpoints, including request/response types.
pub mod rpc;

/// Module for Arkiv client functionality.
/// Exposes the main client interface for interacting with the Arkiv network.
pub mod client;

/// Module for account management.
/// Includes types and helpers for user accounts and key handling.
pub mod account;

/// Module for Arkiv entities and data types.
/// Defines core types such as annotations, hashes, and entity representations.
pub mod entity;

/// Module for event handling.
/// Contains types and utilities for working with Arkiv events.
pub mod events;

/// Module for custom signers.
/// Provides abstractions and implementations for signing transactions and messages.
pub mod signers;

/// Module with utility functions.
/// Includes helpers for encoding, decoding, and other common tasks.
pub mod utils;

/// Module for resilient provider functionality.
/// Provides a wrapper around DynProvider that handles "error sending request" errors by retrying.
pub mod resilient_provider;
