# syntax=docker/dockerfile:1

# ---------- Builder ----------
FROM rust:1-bookworm AS builder

WORKDIR /build

RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config \
        libssl-dev \
        zlib1g-dev \
        libssh2-1-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release --locked

# ---------- Runtime ----------
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
        libssl3 \
        zlib1g \
        libssh2-1 \
        ca-certificates \
        git \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/gitlumina /usr/local/bin/gitlumina
COPY --from=builder /build/target/release/lumina    /usr/local/bin/lumina

WORKDIR /repo

ENTRYPOINT ["lumina"]
