use axum::{
    routing::{get, post},
    Json, Router,
};
use clap::Parser;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncWriteExt, BufReader, AsyncBufReadExt};
use tokio::net::TcpStream;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Spell {
    id: u32,
    name: String,
    description: String,
}

#[derive(Deserialize)]
struct ErrorResponse {
    error: String,
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
        // Lock the connection to ensure only one command is sent at a time
        let mut connection = self.connection.lock().await;

        // Send command
        connection.write_all(command.as_bytes()).await?;
        connection.flush().await?;

        // Wait for the response, reading up to newline
        let mut reader = BufReader::new(&mut *connection);
        let mut response = Vec::new();

        reader.read_until(b'\n', &mut response).await?;

        // Convert the response from Vec<u8> to a String and trim it
        let response_str = String::from_utf8(response)?.trim().to_string();

        Ok(response_str)
    }
}

async fn get_all_spells(
    db: axum::extract::Extension<Arc<Mutex<Database>>>,
) -> Result<Json<Vec<Spell>>, axum::http::StatusCode> {
    // Lock the database connection and execute the command
    let result = db.lock().await.execute("SELECT * FROM spells").await;

    match result {
        Ok(response) => {
            // Attempt to parse the response as an ErrorResponse first
            if let Ok(error_response) = serde_json::from_str::<ErrorResponse>(&response) {
                eprintln!("Error from database: {}", error_response.error);
                return Err(axum::http::StatusCode::BAD_REQUEST);
            }

            // Otherwise, parse the response as a list of spells
            let spells: Vec<Spell> = serde_json::from_str(&response).map_err(|e| {
                eprintln!("Failed to parse spells: {}", e);
                axum::http::StatusCode::INTERNAL_SERVER_ERROR
            })?;
            println!("Got all spells: {:?}", spells);
            Ok(Json(spells))
        }
        Err(e) => {
            eprintln!("Failed to get all spells: {}", e);
            Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn get_spell_by_id(
    db: axum::extract::Extension<Arc<Mutex<Database>>>,
    axum::extract::Path(id): axum::extract::Path<u32>,
) -> Result<Json<Spell>, axum::http::StatusCode> {
    // Lock the database connection and execute the command
    let result = db.lock().await.execute(&format!("SELECT * FROM spells WHERE id = {}", id)).await;

    match result {
        Ok(response) => {
            // Attempt to parse the response as an ErrorResponse first
            if let Ok(error_response) = serde_json::from_str::<ErrorResponse>(&response) {
                eprintln!("Error from database: {}", error_response.error);
                return Err(axum::http::StatusCode::BAD_REQUEST);
            }

            // Otherwise, parse the response as a list of spell
            let spells: Vec<Spell> = serde_json::from_str(&response).map_err(|e| {
                eprintln!("Failed to parse spells: {}", e);
                axum::http::StatusCode::INTERNAL_SERVER_ERROR
            })?;

            // Ensure only one spell was returned
            if spells.len() != 1 {
                eprintln!("Expected 1 spell, got {}", spells.len());
                return Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
            }
            println!("Got spell by id: {:?}", spells[0]);
            Ok(Json(spells[0].clone()))
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
) -> Result<Json<Spell>, axum::http::StatusCode> {
    // Lock the database connection and execute the command
    let result = db.lock().await.execute(&format!(
        "INSERT INTO spells (id, name, description) VALUES ({}, '{}', '{}')",
        spell.id, spell.name, spell.description
    )).await;

    match result {
        Ok(response) => {
            // Attempt to parse the response as an ErrorResponse first
            if let Ok(error_response) = serde_json::from_str::<ErrorResponse>(&response) {
                eprintln!("Error from database: {}", error_response.error);
                return Err(axum::http::StatusCode::BAD_REQUEST);
            }
            println!("Created spell: {:?}", spell);
            Ok(Json(spell))
        }
        Err(e) => {
            eprintln!("Failed to create spell: {}", e);
            Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
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
