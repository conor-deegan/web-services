use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::fs;
use tokio::io::{self, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{self, Duration};

#[derive(Deserialize)]
struct Config {
    targets: Vec<Targets>,
}

#[derive(Deserialize, Clone, PartialEq, Eq, Hash, Debug)]
struct Targets {
    address: String,
    health_check_endpoint: String,
}

async fn handle_connection(
    mut incoming: TcpStream,
    backend_address: String,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Forwarding connection to backend: {}", backend_address);

    let mut backend = TcpStream::connect(backend_address).await?;
    let (mut ri, mut wi) = incoming.split();
    let (mut rb, mut wb) = backend.split();

    let client_to_server = io::copy(&mut ri, &mut wb);
    let server_to_client = io::copy(&mut rb, &mut wi);

    tokio::try_join!(client_to_server, server_to_client)?;
    Ok(())
}

async fn check_targets_health(
    target_health: Arc<Mutex<HashMap<Targets, bool>>>,
    targets: Vec<Targets>,
) {
    let client = Client::new();

    for target in &targets {
        let health_check_url = format!("http://{}{}", target.address, target.health_check_endpoint);

        let health = client
            .get(&health_check_url)
            .send()
            .await
            .map(|resp| resp.status().is_success())
            .unwrap_or(false);

        // Update the health status within the lock's scope to minimize lock duration.
        target_health.lock().unwrap().insert(target.clone(), health);
    }

    // Print the number of healthy targets
    println!(
        "Healthy targets: {:?}",
        target_health
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, &v)| v)
            .count()
    );
}

async fn write_flush_shutdown(
    mut socket: tokio::net::TcpStream,
    response: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    socket.write_all(response).await?;
    socket.flush().await?;
    socket.shutdown().await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // read the config file
    let config_str = fs::read_to_string("src/config.toml").await?;
    let Config { targets } = toml::from_str(&config_str)?;

    // Create a map of target health statuses
    let target_health = Arc::new(Mutex::new(HashMap::new()));
    for target in &targets {
        target_health.lock().unwrap().insert(target.clone(), true);
    }

    // Check the health of the targets every 5 seconds
    let target_health_clone = target_health.clone();
    let targets_clone = targets.clone();
    tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await;
            check_targets_health(target_health_clone.clone(), targets_clone.clone()).await;
        }
    });

    // listen for incoming connections to the load balancer on port 80
    let listener = TcpListener::bind("0.0.0.0:80").await?;
    let current_backend = Arc::new(Mutex::new(0));

    println!("Load Balancer running on: {}", listener.local_addr()?);

    loop {
        let (socket, _) = listener.accept().await?;
        let targets_clone = targets.clone();
        let targets_health_clone = target_health.clone();
        let current_backend_clone = current_backend.clone();

        tokio::spawn(async move {
            let healthy_backends = {
                let locked_health = targets_health_clone.lock().unwrap();
                targets_clone
                    .iter()
                    .filter(|b| *locked_health.get(b).unwrap())
                    .collect::<Vec<_>>()
            }; // Lock is dropped here as it goes out of scope.

            if !healthy_backends.is_empty() {
                // Determine the backend to use in a separate, lock-scoped block to avoid capturing the guard.
                let (backend_address, _) = {
                    let mut index_lock = current_backend_clone.lock().unwrap();
                    let index = *index_lock % healthy_backends.len();
                    let backend_address = healthy_backends[index].address.clone();
                    *index_lock += 1;
                    (backend_address, *index_lock)
                }; // Lock is dropped here.

                // Perform the connection handling.
                if let Err(e) = handle_connection(socket, backend_address).await {
                    eprintln!("Failed to handle connection: {}", e);
                }
            } else {
                eprintln!("No healthy backends available.");
                // return an error from the load balancer to the client
                let body = "Service Unavailable";
                let response = format!(
                    "HTTP/1.1 503 Service Unavailable\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                if let Err(e) = write_flush_shutdown(socket, response.as_bytes()).await {
                    eprintln!("Error handling socket: {}", e);
                }
            }
        });
    }
}
