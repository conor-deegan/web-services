use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use std::error::Error;

async fn handle_client(mut socket: TcpStream) {
    let mut buf = vec![0; 1024];
    loop {
        let bytes_read = match socket.read(&mut buf).await {
            // Socket closed
            Ok(n) if n == 0 => return,
            Ok(n) => n,
            Err(e) => {
                eprintln!("Failed to read from socket; err = {:?}", e);
                return;
            },
        };

        // Convert the buffer into a string
        let command = match String::from_utf8(buf[..bytes_read].to_vec()) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Failed to convert buffer to string; err = {:?}", e);
                return;
            },
        };

        println!("Received command: {}", command);

       // Echo everything back to the client
       let write = socket.write_all(command.as_bytes()).await;
       match write {
           Ok(_) => println!("Sent response: {}", command),
           Err(e) => eprintln!("Failed to write to socket; err = {:?}", e),
       }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let listener = TcpListener::bind("0.0.0.0:8004").await?;
    println!("DB listening on port {}", listener.local_addr()?);

    // Accept connections in a loop
    loop {
        let (socket, _) = listener.accept().await?;
        tokio::spawn(async move {
            handle_client(socket).await;
        });
    }
}
