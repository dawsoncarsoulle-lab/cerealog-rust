FROM rust:slim-bookworm AS builder
WORKDIR /usr/src/sap-extractor

RUN apt-get update && apt-get install -y pkg-config libssl-dev

COPY . .

RUN cargo build --release

FROM debian:bookworm-slim
WORKDIR /app

RUN apt-get update && apt-get install -y libssl3 ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/src/sap-extractor/target/release/sap-extractor /usr/local/bin/sap-extractor

CMD ["sap-extractor"]
