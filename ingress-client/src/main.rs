use clap::Parser;
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue},
    Method,
};
use std::{error::Error, net::Ipv4Addr};
use tokio::net::UdpSocket;
use url::Url;

#[derive(Parser, Debug)]
#[clap(version = "0.1.0", author = "Conor Deegan")]
struct Args {
    /// Sets the method for the request
    #[clap(
        short = 'X',
        long = "request",
        value_name = "METHOD",
        default_value = "GET"
    )]
    method: String,

    /// Sets the HTTP request headers
    #[clap(short = 'H', long = "header", value_name = "HEADER")]
    headers: Vec<String>,

    /// Sets the HTTP request body
    #[clap(short = 'd', long = "data", value_name = "DATA")]
    data: Option<String>,

    /// Sets the endpoint to request
    #[clap(value_name = "ENDPOINT")]
    endpoint: String,
}

// Query the DNS resolver for the IP address of a domain
async fn query_dns_resolver(domain: &str) -> Result<Ipv4Addr, Box<dyn Error>> {
    // Connect to the DNS resolver
    let resolver_addr = "127.0.0.1:5354";
    let socket = UdpSocket::bind("127.0.0.1:0").await?;
    socket.connect(resolver_addr).await?;

    // Construct the DNS query message
    let mut query = Vec::with_capacity(512);
    query.extend_from_slice(&[0x00, 0x01]); // Transaction ID
    query.extend_from_slice(&[0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]); // Flags and Counts
    for label in domain.split('.') {
        query.push(label.len() as u8);
        query.extend_from_slice(label.as_bytes());
    }
    query.push(0); // end of domain name
    query.extend_from_slice(&[0x00, 0x01, 0x00, 0x01]); // QType and QClass

    socket.send(&query).await?;

    // Receive the DNS response
    let mut response = [0u8; 512];
    let _ = socket.recv(&mut response).await?;

    // Check for NXDOMAIN response
    // The RCODE is the last four bits of the second byte of the flags section
    // which itself is the second and third bytes of the response
    let rcode = response[3] & 0x0F;
    if rcode == 3 {
        // NXDOMAIN
        return Err("NXDOMAIN: The domain name does not exist.".into());
    }

    let ip_start = 14 + (domain.len() + 2) + 4 + 10; // Skip to the answer part
    let ip_address = Ipv4Addr::new(
        response[ip_start],
        response[ip_start + 1],
        response[ip_start + 2],
        response[ip_start + 3],
    );
    Ok(ip_address)
}

fn extract_host(url_str: &str) -> Result<String, &'static str> {
    let url = Url::parse(url_str).map_err(|_| "Failed to parse URL")?;
    url.host_str()
        .map(|s| s.to_string())
        .ok_or("URL does not contain a host")
}

fn replace_host_with_ip(url_str: &str, ip: Ipv4Addr) -> String {
    let mut url = Url::parse(url_str).unwrap();
    url.set_host(Some(ip.to_string().as_str())).unwrap();
    url.to_string()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Args = Args::parse();

    // Extract the host from the URL
    let host = extract_host(&args.endpoint)?;

    // Print the host
    println!("Host: {}", host);

    // Query the DNS resolver for the IP address of the host
    let ip = query_dns_resolver(&host).await?;

    // Print the IP address
    println!("IP Address: {}", ip);

    // Handle the headers
    let mut headers = HeaderMap::new();
    for header in &args.headers {
        let parts: Vec<&str> = header.splitn(2, ':').collect();
        if parts.len() == 2 {
            let header_name = parts[0].trim();
            let header_value = parts[1].trim();

            if let Ok(h_name) = HeaderName::from_bytes(header_name.as_bytes()) {
                if let Ok(h_value) = HeaderValue::from_str(header_value) {
                    headers.insert(h_name, h_value);
                } else {
                    eprintln!("Invalid header value: {}", header_value);
                }
            } else {
                eprintln!("Invalid header name: {}", header_name);
            }
        }
    }

    // Send the HTTP request
    let client = reqwest::Client::new();
    let request = client
        .request(
            Method::from_bytes(args.method.as_bytes()).unwrap(),
            replace_host_with_ip(&args.endpoint, ip),
        )
        .headers(headers.clone())
        .body(args.data.clone().unwrap_or_default());

    println!("Requesting: {}", args.endpoint);
    println!("Method: {}", args.method);
    println!("Headers: {:?}", headers);
    println!("Payload: {:?}", &args.data);

    let response = request.send().await?;

    if response.status().is_success() {
        let body = response.text().await?;
        println!("Response: {}", body);
    } else {
        eprintln!("Error: {}", response.status());
    }

    Ok(())
}
