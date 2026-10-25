FROM rust:1.99.0-trixie AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY data ./data
RUN cargo build --release --locked

FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 app
COPY --from=build /app/target/release/olaninc-api /usr/local/bin/olaninc-api
USER app
EXPOSE 3000
CMD ["olaninc-api"]
