FROM rust:1-bookworm
RUN rustup target add wasm32-unknown-unknown \
    && cargo install --locked wasm-pack --version 0.15.1
WORKDIR /workspace
