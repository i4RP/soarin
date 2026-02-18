FROM rust:1.93-slim AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src/ src/

RUN apt-get update && apt-get install -y pkg-config && rm -rf /var/lib/apt/lists/*
RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/soarin-backend /usr/local/bin/soarin-backend

ENV HOST=0.0.0.0
ENV PORT=8080
EXPOSE 8080

CMD ["soarin-backend"]
