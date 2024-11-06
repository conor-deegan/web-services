# Start with the official Rust image.
FROM rust:latest

# Install cargo-watch
RUN cargo install cargo-watch

# Set the working directory inside the container to /usr/src/myapp.
WORKDIR /usr/src/myapp

# Copy the Cargo.toml and Cargo.lock files to the container
COPY Cargo.toml ./

# Cache dependencies
RUN cargo fetch

# Copy the current directory contents into the container at /usr/src/myapp.
COPY . .

# Build your application.
RUN cargo build

# Command to run when starting the container.
CMD ["cargo", "watch", "-x", "run"]
