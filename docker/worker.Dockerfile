FROM rust:1.92-bookworm AS builder
WORKDIR /src
COPY . .
RUN cargo build --release --package nopager-worker

FROM docker:27.5.1-cli-alpine3.21 AS docker-cli

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 nopager
COPY --from=docker-cli /usr/local/bin/docker /usr/local/bin/docker
COPY --from=builder /src/target/release/nopager-worker /usr/local/bin/nopager-worker
USER nopager
ENTRYPOINT ["nopager-worker"]
