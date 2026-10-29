FROM rust:1.99.0-trixie AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl gosu \
    && rm -rf /var/lib/apt/lists/* && useradd --system --uid 10002 monitor
COPY --from=build /app/target/release/cost-monitor /usr/local/bin/cost-monitor
COPY entrypoint.sh /usr/local/bin/cost-monitor-entrypoint
ENTRYPOINT ["sh", "/usr/local/bin/cost-monitor-entrypoint"]
