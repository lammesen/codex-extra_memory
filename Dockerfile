FROM lukemathwalker/cargo-chef:latest-rust-1.85-bookworm@sha256:58b733252ce21d8870575205d803c24c108b550a033a826b51c2d8fb7ed16e1b AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json --locked
COPY . .
RUN cargo build --release --locked -p codex-extra-memory-mcp

FROM debian:bookworm-slim@sha256:98f4b71de414932439ac6ac690d7060df1f27161073c5036a7553723881bffbe AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*
RUN useradd --create-home appuser
USER appuser
WORKDIR /home/appuser
COPY --from=builder /app/target/release/codex-extra-memory-mcp /usr/local/bin/codex-extra-memory-mcp
ENTRYPOINT ["codex-extra-memory-mcp"]
