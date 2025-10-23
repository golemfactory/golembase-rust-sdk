use actix_web::{web, HttpResponse, Responder};
use arkiv_sdk::entity::{Create, Entity, Update};
use arkiv_sdk::{Address, ArkivClient, Hash};
use sqlx::Row;
use sqlx::SqlitePool;

pub async fn create_entity(
    client: web::Data<ArkivClient>,
    db_pool: web::Data<SqlitePool>,
    item: web::Json<Create>,
) -> impl Responder {
    let create = item.into_inner();
    match client.create_entities(vec![create.clone()]).await {
        Ok(receipts) => {
            for receipt in &receipts {
                let _ = sqlx::query("INSERT OR REPLACE INTO entities (id, data) VALUES (?, ?)")
                    .bind(receipt.entity_key.to_string())
                    .bind(serde_json::to_string(&create.data).unwrap())
                    .execute(db_pool.get_ref())
                    .await;
            }
            HttpResponse::Created().json(receipts)
        }
        Err(e) => HttpResponse::InternalServerError().body(format!("Error creating entity: {}", e)),
    }
}

pub async fn get_entities(
    client: web::Data<ArkivClient>,
    db_pool: web::Data<SqlitePool>,
    owner_address: web::Path<Address>,
) -> impl Responder {
    let rows = sqlx::query("SELECT id, data FROM entities WHERE owner = ?")
        .bind(owner_address.to_string())
        .fetch_all(db_pool.get_ref())
        .await
        .ok();
    if let Some(rows) = rows {
        let mut entities = Vec::new();
        for row in rows {
            let _id: String = row.get("id");
            let data: String = row.get("data");
            if let Ok(entity) = serde_json::from_str::<Entity>(&data) {
                entities.push(entity);
            }
        }
        if !entities.is_empty() {
            return HttpResponse::Ok().json(entities);
        }
    }
    match client
        .get_entities_of_owner(owner_address.into_inner())
        .await
    {
        Ok(entities) => HttpResponse::Ok().json(entities),
        Err(e) => {
            HttpResponse::InternalServerError().body(format!("Error fetching entities: {}", e))
        }
    }
}

pub async fn update_entity(
    client: web::Data<ArkivClient>,
    db_pool: web::Data<SqlitePool>,
    item: web::Json<Update>,
) -> impl Responder {
    let update = item.into_inner();
    match client.update_entities(vec![update.clone()]).await {
        Ok(_receipts) => {
            let _ = sqlx::query("UPDATE entities SET data = ? WHERE id = ?")
                .bind(serde_json::to_string(&update.data).unwrap())
                .bind(update.entity_key.to_string())
                .execute(db_pool.get_ref())
                .await;
            HttpResponse::Ok().body("Entity updated successfully")
        }
        Err(e) => HttpResponse::InternalServerError().body(format!("Error updating entity: {}", e)),
    }
}

pub async fn delete_entity(
    client: web::Data<ArkivClient>,
    db_pool: web::Data<SqlitePool>,
    entity_key: web::Path<String>,
) -> impl Responder {
    let key: Hash = entity_key
        .into_inner()
        .parse()
        .expect("Invalid entity key format");
    match client.delete_entities(vec![key]).await {
        Ok(_) => {
            let _ = sqlx::query("DELETE FROM entities WHERE id = ?")
                .bind(key.to_string())
                .execute(db_pool.get_ref())
                .await;
            HttpResponse::Ok().body("Entity deleted successfully")
        }
        Err(e) => HttpResponse::InternalServerError().body(format!("Error deleting entity: {}", e)),
    }
}

pub fn init_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("")
            .route("/entities", web::post().to(create_entity))
            .route("/entities/{id}", web::get().to(get_entities))
            .route("/entities/{id}", web::put().to(update_entity))
            .route("/entities/{id}", web::delete().to(delete_entity)),
    );
}
