# Start with the official Rust image.
FROM rust:latest

# Set the working directory inside the container to /usr/src/myapp.
WORKDIR /usr/src/myapp

# Copy the current directory contents into the container at /usr/src/myapp.
COPY . .

# Build your application.
RUN cargo build

# Command to run when starting the container.
CMD ["cargo", "run"]
