use axum::{
    routing::{get, post},
    Json, Router,
};
use reqwest::Client;
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
struct CacheStore {
    client: Client,
    base_url: String,
}

#[derive(Clone)]
struct MessageQueue {
    client: Client,
    base_url: String,
}

#[derive(Clone)]
struct Database {
    connection: Arc<Mutex<TcpStream>>,
}

impl CacheStore {
    fn new(base_url: &str) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.to_string(),
        }
    }

    // Retrieves a value from the cache store by key
    async fn get(&self, key: &str) -> Result<Option<String>, Box<dyn std::error::Error>> {
        let url = format!("{}/get/{}", self.base_url, key);
        
        let response = self.client.get(&url).send().await?;
        
        // Check if the response is 404, meaning the key doesn't exist
        if response.status().as_u16() == 404 {
            return Ok(None); // Key not found in cache
        }

        // Parse the response as JSON
        let value = response.text().await?;
        Ok(Some(value))
    }

    // Sets a key-value pair in the cache store
    async fn set(&self, key: &str, value: &str) -> Result<(), Box<dyn std::error::Error>> {
        let url = format!("{}/set", self.base_url);

        let params = serde_json::json!({ "key": key, "value": value });
        
        // best effort, ignore errors
        self.client.post(&url)
            .json(&params)
            .send()
            .await?;

        Ok(())
    }
}

impl MessageQueue {
    fn new(base_url: &str) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.to_string(),
        }
    }

    // Sets a key-value pair in the cache store
    async fn enqueue(&self, message: &str) -> Result<(), Box<dyn std::error::Error>> {
        let url = format!("{}/enqueue", self.base_url);

        let params = serde_json::json!({ "message": message });
        
        // best effort, ignore errors
        self.client.post(&url)
            .json(&params)
            .send()
            .await?;

        Ok(())
    }
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
    cache: axum::extract::Extension<Arc<CacheStore>>,
    axum::extract::Path(id): axum::extract::Path<u32>,
) -> Result<Json<Spell>, axum::http::StatusCode> {

    // Try to get the spell from the cache first
    let cached_spell = cache.get(&id.to_string()).await.map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(cached_spell) = cached_spell {
        println!("Cache hit for spell by id: {}", id);
        let spell: Spell = serde_json::from_str(&cached_spell).map_err(|e| {
            eprintln!("Failed to parse cached spell: {}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;
        return Ok(Json(spell));
    }

    println!("Cache miss for spell by id: {}", id);

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

            // Spawn a background task to cache the response; ignore errors from `set`
            let cache = cache.clone();
            let spell_id = id.to_string();
            let spell_data = serde_json::to_string(&spells[0]).unwrap();
            tokio::spawn(async move {
                if let Err(e) = cache.set(&spell_id, &spell_data).await {
                    eprintln!("Failed to cache spell by id: {}, error: {}", spell_id, e);
                }
            });

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
    message_queue: axum::extract::Extension<Arc<MessageQueue>>,
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

            // Spawn a background task to enqueue the response; ignore errors from `enqueue`
            let message_queue = message_queue.clone();
            let spell_data = serde_json::to_string(&spell).unwrap();
            tokio::spawn(async move {
                if let Err(e) = message_queue.enqueue(&spell_data).await {
                    eprintln!("Failed to enqueue spell: {}, error: {}", spell.id, e);
                }
            });
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

    // Connect to the cache store
    let cache = CacheStore::new("http://cache-store:8005");

    // Connect to the message queue
    let message_queue = MessageQueue::new("http://message-queue:8006");

    let app = Router::new()
        .route("/api/spells", get(get_all_spells))
        .route("/api/spells", post(create_spell))
        .route("/api/spells/:id", get(get_spell_by_id))
        .route("/healthz", get(|| async { "OK" }))
        .layer(axum::extract::Extension(Arc::new(Mutex::new(db))))
        .layer(axum::extract::Extension(Arc::new(cache)))
        .layer(axum::extract::Extension(Arc::new(message_queue)));

    // Start the server
    let listener =
        tokio::net::TcpListener::bind(format!("{}:{}", config.host, config.port)).await?;
    println!("Server running on http://{}:{}", config.host, config.port);
    axum::serve(listener, app).await.unwrap();

    Ok(())
}
