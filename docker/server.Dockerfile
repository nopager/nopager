FROM rust:1.92-bookworm AS builder
WORKDIR /src
COPY . .
RUN cargo build --release --package nopager-server

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 nopager
COPY --from=builder /src/target/release/nopager-server /usr/local/bin/nopager-server
USER nopager
EXPOSE 8080
ENTRYPOINT ["nopager-server"]
