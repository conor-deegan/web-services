use axum::{
    routing::{get, post},
    Json, Router,
};
use clap::Parser;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use tokio::net::TcpStream;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Spell {
    id: u32,
    name: String,
    description: String,
}

#[derive(Clone)]
struct Database {
    connection: Arc<Mutex<TcpStream>>,
}

impl Database {
    async fn new(connection_string: &str) -> Result<Self, Box<dyn std::error::Error>> {
        loop {
            match TcpStream::connect(connection_string).await {
                Ok(connection) => {
                    println!("Connected to DB at {:?}", connection.peer_addr()?);
                    let database = Self {
                        connection: Arc::new(Mutex::new(connection))
                    };
                    return Ok(database);
                },
                Err(e) => {
                    eprintln!("Failed to connect to DB: {}", e);
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        }
    }

    async fn execute(&self, command: &str) -> Result<String, Box<dyn std::error::Error>> {
        let command = format!("{}\0", command); // append delimiter to command
        let mut lock = self.connection.lock().await;
        lock.write_all(command.as_bytes()).await?;
        let mut response = Vec::new();
        let mut buffer = [0; 1024];
        let delimiter = b'\0'; // Null byte as delimiter

        loop {
            let n = lock.read(&mut buffer).await?;
            if n == 0 || buffer[..n].contains(&delimiter) {
                break;
            } // End of message or stream
            response.extend_from_slice(&buffer[..n]);
        }
        // Remove the delimiter from the response if present
        if let Some(pos) = response.iter().position(|&x| x == delimiter) {
            response.truncate(pos);
        }

        // Convert the response to a string
        let response = String::from_utf8(response)?;
        Ok(response)
    }
}

async fn get_all_spells(db: axum::extract::Extension<Arc<Mutex<Database>>>,) -> Result<Json<Vec<Spell>>, axum::http::StatusCode> {
    let res = db.lock().await.execute("SELECT * FROM spells").await;
    match res {
        Ok(response) => {
            let spells: Vec<Spell> = serde_json::from_str(&response).unwrap();
            Ok(Json(spells))
        }
        Err(e) => {
            eprintln!("Failed to get all spells: {}", e);
            Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn get_spell_by_id(
    id: axum::extract::Path<u32>,
    db: axum::extract::Extension<Arc<Mutex<Database>>>,
) -> Result<Json<Spell>, axum::http::StatusCode> {
    println!("Getting spell by id: {}", id.0);
    let res = db.lock().await.execute(&format!("SELECT * FROM spells WHERE id = {}", id.0)).await;
    match res {
        Ok(response) => {
            let spell: Spell = serde_json::from_str(&response).unwrap();
            Ok(Json(spell))
        }
        Err(e) => {
            eprintln!("Failed to get spell by id: {}", e);
            Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn create_spell(
    db: axum::extract::Extension<Arc<Mutex<Database>>>,
    Json(spell): Json<Spell>,
) -> Json<Spell> {
    println!("Creating spell: {:?}", spell);
    let res = db.lock().await.execute(&format!(
        "INSERT INTO spells (id, name, description) VALUES ({}, '{}', '{}')",
        spell.id, spell.name, spell.description
    )).await;
    match res {
        Ok(_) => Json(spell),
        Err(e) => {
            eprintln!("Failed to create spell: {}", e);
            Json(Spell {
                id: 0,
                name: "Failed to create spell".to_string(),
                description: e.to_string(),
            })
        }
    }
}

#[derive(Parser)]
pub struct Config {
    #[arg(short = 'H', long, default_value = "0.0.0.0")]
    pub host: String,

    #[arg(short, long, default_value_t = 8001)]
    pub port: u16,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::parse();

    // Connect to the database over long-lived TCP connection
    let db = Database::new("data-store:8004").await?;

    let app = Router::new()
        .route("/api/spells", get(get_all_spells))
        .route("/api/spells", post(create_spell))
        .route("/api/spells/:id", get(get_spell_by_id))
        .route("/healthz", get(|| async { "OK" }))
        .layer(axum::extract::Extension(Arc::new(Mutex::new(db))));

    // Start the server
    let listener =
        tokio::net::TcpListener::bind(format!("{}:{}", config.host, config.port)).await?;
    println!("Server running on http://{}:{}", config.host, config.port);
    axum::serve(listener, app).await.unwrap();

    Ok(())
}
