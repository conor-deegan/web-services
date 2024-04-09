use tokio::net::UdpSocket;
use std::error::Error;
use std::net::Ipv4Addr;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

struct CacheEntry {
    ip_address: Ipv4Addr,
    valid_until: u64,
}

struct DnsCache {
    entries: HashMap<String, CacheEntry>,
}

// simple DNS Cache implementation
impl DnsCache {
    fn new() -> Self {
        DnsCache { entries: HashMap::new() }
    }

    fn get(&self, domain: &str) -> Option<(Ipv4Addr, u64)> {
        if let Some(entry) = self.entries.get(domain) {
            // Check if the entry is still valid
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
            if entry.valid_until > now {
                return Some((entry.ip_address, entry.valid_until));
            }
        }
        None
    }

    fn insert(&mut self, domain: &str, ip_address: Ipv4Addr, ttl: u32) {
        let valid_until = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() + u64::from(ttl);
        self.entries.insert(domain.to_string(), CacheEntry { ip_address, valid_until });
    }
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

// Query the authoritative DNS server for the IP address of a domain if not found in the cache.
async fn query_authoritative_server(domain: &str) -> Result<(Ipv4Addr, u32), Box<dyn Error>> {
    // Connect to the authoritative DNS server
    let server_addr = "dns-server:53";
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    socket.connect(server_addr).await?;

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

    // TTL is 6 bytes before the IP address in the answer
    let ttl_bytes = &response[ip_start - 6..ip_start - 2];
    let ttl = u32::from_be_bytes(ttl_bytes.try_into()?);

    println!("Resolved {} to {} with TTL {}", domain, ip_address, ttl);

    Ok((ip_address, ttl))
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
    let resolver_socket = UdpSocket::bind("0.0.0.0:5354").await?;
    println!("DNS Resolver listening on {}", resolver_socket.local_addr()?);

    let mut cache = DnsCache::new();

    let mut request = [0u8; 512];

    loop {
        let (_, client_addr) = resolver_socket.recv_from(&mut request).await?;
        println!("Received query from {}", client_addr);

        match parse_domain_name(&request, 12) {
            Ok(domain) => {
                 println!("Parsed domain: {}", domain);

                 // Check if the domain is in the cache
                 if let Some((ip_address, ttl)) = cache.get(&domain) {
                      // Send the cached IP address to the client
                      println!("Cache hit: {} -> {}", domain, ip_address);
                      let transaction_id = [request[0], request[1]];
                      let ttl_u32 = ttl as u32; // Convert u16 to u32
                      let response = create_dns_response(transaction_id, &domain, ip_address, ttl_u32);
                      if let Err(e) = resolver_socket.send_to(&response, &client_addr).await {
                          eprintln!("Failed to send response: {}", e);
                      } else {
                          println!("Sent response to {} for domain {} and ip {}", client_addr, domain, ip_address);
                      }
                  } else {
                      // Query the authoritative server for the IP address
                      match query_authoritative_server(&domain).await {
                          Ok((ip_address, ttl)) => {
                              println!("Cache miss: {} -> {} {}", domain, ip_address, ttl);
                              // Insert the domain and IP address into the cache
                              cache.insert(&domain, ip_address, ttl);

                              let transaction_id = [request[0], request[1]];
                              let response = create_dns_response(transaction_id, &domain, ip_address, ttl);
                              if let Err(e) = resolver_socket.send_to(&response, &client_addr).await {
                                  eprintln!("Failed to send response: {}", e);
                              } else {
                                  println!("Sent response to {} for domain {} and ip {}", client_addr, domain, ip_address);
                              }
                          },
                          Err(_e) => {
                              // Send a NXDOMAIN response to the client
                              let transaction_id = [request[0], request[1]];
                              if let Err(e) = send_nxdomain_response(transaction_id, &request, request.len(), &client_addr, &resolver_socket).await {
                                  eprintln!("Failed to send NXDOMAIN response: {}", e);
                              } else {
                                  println!("Sent NXDOMAIN response to {}", client_addr);
                              }
                          }
                      }
                  }
            },
            Err(e) => eprintln!("Failed to parse domain name: {}", e),
        }
    }
}
