use actix_web::{web, App, HttpServer};
use arkiv_sdk::{ArkivClient, Hash, PrivateKeySigner, Url};
use clap::Parser;
use dirs::config_dir;
use sqlx::SqlitePool;
use tokio::fs;

mod api;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(short, long, default_value = "http://localhost:8545")]
    rpc_url: String,
}

#[actix_web::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let rpc_url = Url::parse(&cli.rpc_url).expect("Invalid URL format");

    let mut private_key_path = config_dir().ok_or("Failed to get config directory")?;
    private_key_path.push("arkiv/private.key");

    let private_key_bytes = fs::read(&private_key_path).await?;
    let private_key = Hash::from_slice(&private_key_bytes);

    let signer = PrivateKeySigner::from_bytes(&private_key)
        .map_err(|e| format!("Failed to parse private key: {}", e))?;

    let client = ArkivClient::builder()
        .wallet(signer)
        .rpc_url(rpc_url)
        .build();

    let db_pool = SqlitePool::connect("sqlite::memory:").await?;

    // Ensure the entities table exists
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS entities (
            id TEXT PRIMARY KEY,
            owner TEXT NOT NULL,
            data TEXT NOT NULL
        )
        "#,
    )
    .execute(&db_pool)
    .await?;

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(db_pool.clone()))
            .app_data(web::Data::new(client.clone()))
            .configure(api::init_routes)
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await?;

    Ok(())
}
