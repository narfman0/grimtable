# One image: the server binary, the built client and the data it reads.
# Built from the checkout with the submodules initialised
# (`git submodule update --init`); `.dockerignore` keeps the rest out.

# --- Rust: the server and the wasm engine ---------------------------------
FROM rust:1-bookworm AS rust
RUN rustup target add wasm32-unknown-unknown \
 && curl -sSf https://rustwasm.github.io/wasm-pack/installer/init.sh | sh
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p server \
 && cp target/release/server /usr/local/bin/grimtable-server \
 && wasm-pack build crates/wasm --target web --out-dir /wasm

# --- Client ---------------------------------------------------------------
FROM node:22-bookworm-slim AS client
WORKDIR /src
COPY scripts scripts
COPY data data
COPY crates/roster/fixtures crates/roster/fixtures
COPY client/package.json client/package-lock.json client/
RUN cd client && npm ci
COPY client client
COPY --from=rust /wasm client/src/wasm
RUN ./scripts/sync-data.sh && cd client && npm run build

# --- Runtime --------------------------------------------------------------
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
# The server builds faction catalogues from these on first request.
COPY data/40kdc-data/data data/40kdc-data/data
COPY data/wh40k-11e/*.json data/wh40k-11e/
COPY --from=client /src/client/dist client/dist
COPY --from=rust /usr/local/bin/grimtable-server /usr/local/bin/grimtable-server
ENV GRIMTABLE_ROOT=/app \
    GRIMTABLE_ADDR=0.0.0.0:8080 \
    RUST_LOG=info,tower_http=info
EXPOSE 8080
CMD ["grimtable-server"]
