# syntax=docker/dockerfile:1

# ---------- Builder ----------
FROM rust:1-bookworm AS builder

WORKDIR /build

# OpenSSL, libgit2, and libssh2 are vendored via git2's `vendored-openssl`
# and `vendored-libgit2` features. We only need cmake (for libgit2) and
# perl (for OpenSSL's build); make/gcc are already in rust:1-bookworm.
RUN apt-get update && apt-get install -y --no-install-recommends \
        cmake \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release --locked

# ---------- Runtime ----------
FROM debian:bookworm-slim

# Only ca-certificates is needed at runtime — OpenSSL, libgit2, and libssh2
# are statically linked into the binary.
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/gitlumina /usr/local/bin/gitlumina
COPY --from=builder /build/target/release/lumina    /usr/local/bin/lumina

# Trust any bind-mounted repo regardless of its host UID/GID.
# Required because libgit2 refuses to open repos not owned by the current
# user (CVE-2022-24765), and the host UID rarely matches root inside the image.
# Written directly to /etc/gitconfig so we don't need the git CLI in the image.
RUN printf '[safe]\n\tdirectory = *\n' > /etc/gitconfig

WORKDIR /repo

ENTRYPOINT ["lumina"]
