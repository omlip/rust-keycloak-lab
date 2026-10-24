FROM rust:1.99.0-trixie AS build
WORKDIR /app/cost-monitor
COPY cost-monitor/Cargo.toml cost-monitor/Cargo.lock ./
COPY cost-monitor/src ./src
COPY cost-monitor/resources ./resources
RUN cargo build --release --locked

FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl gosu \
    && rm -rf /var/lib/apt/lists/* && useradd --system --uid 10002 monitor
COPY --from=build /app/cost-monitor/target/release/cost-monitor /usr/local/bin/cost-monitor
COPY cost-monitor/entrypoint.sh /usr/local/bin/cost-monitor-entrypoint
ENTRYPOINT ["sh", "/usr/local/bin/cost-monitor-entrypoint"]
