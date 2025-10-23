# Arkiv CLI

A command-line interface tool for interacting with Arkiv.

## Installation

```bash
cargo install --path .
```

## Usage

To see detailed debug logs, set the `RUST_LOG` environment variable before runnign any command:
```bash
export RUST_LOG=debug
arkiv-sdk-cli <command>
```

The CLI provides several commands for managing accounts and transactions:

### List all accounts and their balances
```bash
arkiv-sdk-cli list
```

### Fund an account
```bash
arkiv-sdk-cli fund [--wallet <WALLET>] [--amount <AMOUNT>]
```

### Transfer ETH between accounts
```bash
arkiv-sdk-cli transfer --from <FROM> --to <TO> --amount <AMOUNT> [--password <PASSWORD>]
```

### Get entity by ID
```bash
arkiv-sdk-cli get-entity <ID>
```

## Configuration

The CLI uses the system's config directory to store account information. On Linux, this is typically `~/.config/arkiv/`.


## Development

To build the project:
```bash
cargo build
```

To run tests:
```bash
cargo test
```
