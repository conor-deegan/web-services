use tokio::net::UdpSocket;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, Error};
use std::path::Path;
use std::net::Ipv4Addr;

// Load A records (domain to IP mappings) and their TTLs from a specified file.
async fn load_a_records_from_file(file_path: &Path) -> io::Result<HashMap<String, (Ipv4Addr, u32)>> {
    let file = File::open(file_path)?;
    let buf = io::BufReader::new(file);
    let mut a_records = HashMap::new();

    for line in buf.lines() {
        // Parsing each line to extract domain, IP address, and TTL.
        let line = line?;
        let parts: Vec<&str> = line.split('=').collect();
        if parts.len() == 2 {
            let domain = parts[0];
            let rest: Vec<&str> = parts[1].split(',').collect();
            if rest.len() == 2 {
                // Converting string IP to Ipv4Addr and string TTL to u32.
                let ip_address = rest[0].parse().map_err(|_| Error::new(io::ErrorKind::InvalidData, "Invalid IP address"))?;
                let ttl = rest[1].parse().map_err(|_| Error::new(io::ErrorKind::InvalidData, "Invalid TTL"))?;
                // Storing the parsed data in a HashMap.
                a_records.insert(domain.to_string(), (ip_address, ttl));
            }
        }
    }

    Ok(a_records)
}

// Construct a DNS response given a domain name, its resolved IP address, and TTL.
fn create_dns_response(transaction_id: [u8; 2], domain: &str, ip_address: Ipv4Addr, ttl: u32) -> Vec<u8> {
    let mut response = Vec::new();
    let ip_bytes = ip_address.octets();
    let ttl_bytes = ttl.to_be_bytes(); // Convert TTL to byte array in big-endian format

    // Transaction ID, Flags, Questions, Answer RRs, Authority RRs, Additional RRs
    response.extend_from_slice(&transaction_id);
    response.extend_from_slice(&[0x81, 0x80, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00]);

    // Question section
    for label in domain.split('.') {
        response.push(label.len() as u8);
        response.extend_from_slice(label.as_bytes());
    }
    response.extend_from_slice(&[0x00, 0x00, 0x01, 0x00, 0x01]);

    // Answer section
    response.extend_from_slice(&[0xc0, 0x0c, 0x00, 0x01, 0x00, 0x01]);
    response.extend_from_slice(&ttl_bytes);
    response.extend_from_slice(&[0x00, 0x04]);
    response.extend_from_slice(&ip_bytes);

    response
}

// Parse the domain name from the DNS query buffer.
fn parse_domain_name(buf: &[u8], start: usize) -> Result<String, &'static str> {
    let mut position = start;
    let mut domain_name = String::new();

    while position < buf.len() && buf[position] != 0 {
        let length = buf[position] as usize;
        position += 1; // move past the length byte

        // Check for potential out-of-bounds or invalid length
        if length == 0 || position + length > buf.len() {
            return Err("Invalid domain name in query");
        }

        if !domain_name.is_empty() {
            domain_name.push('.');
        }
        let label = match std::str::from_utf8(&buf[position..position + length]) {
            Ok(s) => s,
            Err(_) => return Err("Invalid UTF-8 label in domain name"),
        };
        domain_name.push_str(label);

        position += length; // move to the next label
    }

    Ok(domain_name)
}

// Send a DNS response with NXDOMAIN (non-existent domain) to the client.
async fn send_nxdomain_response(
    transaction_id: [u8; 2],
    request: &[u8],
    request_len: usize,
    addr: &std::net::SocketAddr,
    socket: &tokio::net::UdpSocket,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut response = Vec::new();

    // Transaction ID
    response.extend_from_slice(&transaction_id);

    // Flags: Response, Opcode 0 (Standard Query), Authoritative Answer False, Truncated False,
    // Recursion Desired True, Recursion Available False, Z Reserved, Answer Authenticated False,
    // Non-authenticated data Acceptable, Reply Code NXDOMAIN (3)
    response.extend_from_slice(&[0x81, 0x83]); // Note: 0x83 indicates NXDOMAIN

    // Questions: 1, Answer RRs: 0, Authority RRs: 0, Additional RRs: 0
    response.extend_from_slice(&[0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);

    // Repeat the question section from the request
    response.extend_from_slice(&request[12..request_len]);

    // Sending the NXDOMAIN response
    socket.send_to(&response, addr).await?;

    Ok(())
}


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load the A records from "src/domain.txt".
    let a_records = load_a_records_from_file(Path::new("src/domain.txt")).await?;

    // Bind the server to UDP port 53 and listens for incoming DNS queries.
    let socket = UdpSocket::bind("0.0.0.0:53").await?;
    println!("DNS Server listening on {}", socket.local_addr()?);

    let mut buf = [0u8; 512]; // Buffer to store incoming DNS queries.

    loop {
        let (_, addr) = socket.recv_from(&mut buf).await?;
        println!("Received query from {}", addr);

        match parse_domain_name(&buf, 12) { // Start parsing after the header
            Ok(domain) => {
                println!("Parsed domain: {}", domain);
                match a_records.get(&domain) {
                    Some((ip_address, ttl)) => {
                        let transaction_id = [buf[0], buf[1]];
                        let response = create_dns_response(transaction_id, &domain, *ip_address, *ttl);
                        if let Err(e) = socket.send_to(&response, &addr).await {
                            eprintln!("Failed to send response: {}", e);
                        } else {
                            println!("Sent response to {} for domain {} and ip {}", addr, domain, ip_address);
                        }
                    },
                    None => {
                        let transaction_id = [buf[0], buf[1]];
                        if let Err(e) = send_nxdomain_response(transaction_id, &buf, buf.len(), &addr, &socket).await {
                            eprintln!("Failed to send NXDOMAIN response: {}", e);
                        } else {
                            println!("Sent NXDOMAIN response to {}", addr);
                        }
                    }
                }
            },
            Err(e) => eprintln!("Failed to parse domain name: {}", e),
        }
    }

}
