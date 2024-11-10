use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::fs;
use tokio::io::{self, AsyncWriteExt, AsyncReadExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{self, Duration};

#[derive(Deserialize)]
struct Config {
    targets: Vec<Targets>,
    path_routes: Vec<PathRoute>,
}

#[derive(Deserialize, Clone, PartialEq, Eq, Hash, Debug)]
struct Targets {
    address: String,
    health_check_endpoint: String,
}

#[derive(Deserialize, Clone, PartialEq, Eq, Hash, Debug)]
struct PathRoute {
    path: String,
    address: String,
}

async fn handle_connection(
    mut incoming: TcpStream,
    backend_address: String,
    initial_buffer: &[u8],  // New parameter for the initial buffer
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Forwarding connection to backend: {}", backend_address);

    let mut backend = TcpStream::connect(backend_address).await?;
    let (mut ri, mut wi) = incoming.split();
    let (mut rb, mut wb) = backend.split();

    // Send the initial buffered data to the backend first
    wb.write_all(initial_buffer).await?;

    // Then continue copying the rest of the data
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
    let Config { targets, path_routes } = toml::from_str(&config_str)?;

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
        let (mut socket, _) = listener.accept().await?;
        let targets_clone = targets.clone();
        let targets_health_clone = target_health.clone();
        let current_backend_clone = current_backend.clone();
        let path_routes_clone = path_routes.clone();
    
        tokio::spawn(async move {
            // Read the initial data into a buffer
            let mut buffer = [0; 1024];
            let bytes_read = match socket.read(&mut buffer).await {
                Ok(bytes) => bytes,
                Err(e) => {
                    eprintln!("Failed to read from socket: {}", e);
                    return;
                }
            };
    
            // Parse the request path without consuming the buffer
            let request = String::from_utf8_lossy(&buffer[..bytes_read]);
            let path = if let Some(line) = request.lines().next() {
                line.split_whitespace().nth(1).unwrap_or("/").to_string()
            } else {
                "/".to_string()
            };
            
            println!("Request path: {}", path);
    
            // Check if the path matches any specific route in path_routes
            let backend_address = if let Some(route) = path_routes_clone
                .iter()
                .find(|route| path.starts_with(&route.path))
            {
                println!("Routing to specific path-based backend: {}", route.address);
                route.address.clone()
            } else {
                // Collect the addresses of the path-routed backends
                let path_routed_addresses: Vec<String> = path_routes_clone.iter()
                .map(|route| route.address.clone())
                .collect();

                // Regular load balancing for non-matching paths
                let healthy_backends = {
                    let locked_health = targets_health_clone.lock().unwrap();
                    targets_clone
                        .iter()
                        .filter(|b| *locked_health.get(b).unwrap())
                        .filter(|b| !path_routed_addresses.contains(&b.address))
                        .collect::<Vec<_>>()
                };
    
                if healthy_backends.is_empty() {
                    eprintln!("No healthy backends available.");
                    let body = "Service Unavailable";
                    let response = format!(
                        "HTTP/1.1 503 Service Unavailable\r\nContent-Length: {}\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    if let Err(e) = write_flush_shutdown(socket, response.as_bytes()).await {
                        eprintln!("Error handling socket: {}", e);
                    }
                    return;
                }
    
                // Select a backend using round-robin
                let (address, _) = {
                    let mut index_lock = current_backend_clone.lock().unwrap();
                    let index = *index_lock % healthy_backends.len();
                    let address = healthy_backends[index].address.clone();
                    *index_lock += 1;
                    (address, *index_lock)
                };
    
                address
            };
    
            // Forward the initial buffer along with the rest of the connection to the backend
            if let Err(e) = handle_connection(socket, backend_address, &buffer[..bytes_read]).await {
                eprintln!("Failed to handle connection: {}", e);
            }
        });
    }
}
