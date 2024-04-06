use axum::{
    routing::{get, post},
    Json, Router,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use serde::{Deserialize, Serialize};
use clap::Parser;

#[derive(Serialize, Deserialize, Clone)]
struct Spell {
    id: u32,
    name: String,
    description: String,
}

type Spells = Arc<Mutex<HashMap<u32, Spell>>>;

fn sample_spells() -> HashMap<u32, Spell> {
    let mut spells = HashMap::new();
    spells.insert(
        1,
        Spell {
            id: 1,
            name: "Expelliarmus".to_string(),
            description: "Disarming Charm".to_string(),
        },
    );
    spells.insert(
        2,
        Spell {
            id: 2,
            name: "Lumos".to_string(),
            description: "Creates light at wand tip".to_string(),
        },
    );
    spells
}

async fn get_all_spells(spells: axum::extract::Extension<Spells>) -> Json<Vec<Spell>> {
    let spells = spells.lock().await;
    Json(spells.values().cloned().collect())
}

async fn get_spell_by_id(
    id: axum::extract::Path<u32>,
    spells: axum::extract::Extension<Spells>,
) -> Result<Json<Spell>, axum::http::StatusCode> {
    let spells = spells.lock().await;
    match spells.get(&id.0) {
        Some(spell) => Ok(Json(spell.clone())),
        None => Err(axum::http::StatusCode::NOT_FOUND),
    }
}

async fn create_spell(
    axum::extract::Extension(spells): axum::extract::Extension<Spells>,
    Json(spell): Json<Spell>,
) -> Json<Spell> {
    let mut spells = spells.lock().await;
    spells.insert(spell.id, spell.clone());
    Json(spell)
}

#[derive(Parser)]
pub struct Config {
    #[arg(short = 'H', long, default_value = "127.0.0.1")]
    pub host: String,

    #[arg(short, long, default_value_t = 3000)]
    pub port: u16,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::parse();
    let spells = Arc::new(Mutex::new(sample_spells()));

    let app = Router::new()
        .route("/api/spells", get(get_all_spells))
        .route("/api/spells", post(create_spell))
        .route("/api/spells/:id", get(get_spell_by_id))
        .layer(axum::extract::Extension(spells));

    // Start the server
    let listener = tokio::net::TcpListener::bind(format!("{}:{}", config.host, config.port))
        .await?;
    println!("Server running on http://{}:{}", config.host, config.port);
    axum::serve(listener, app).await.unwrap();

    Ok(())
}
