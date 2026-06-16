FROM rust:1-bookworm AS builder

WORKDIR /app

COPY Cargo.toml ./
COPY macros/Cargo.toml macros/Cargo.toml
COPY macros/src macros/src
COPY src src
COPY templates templates
COPY examples examples

RUN cargo build --release --examples --bin viz_server

FROM debian:bookworm-slim

WORKDIR /app

COPY --from=builder /app/target/release/examples/ /app/examples/
COPY --from=builder /app/target/release/viz_server /usr/local/bin/viz-server
COPY docker-entrypoint.sh /usr/local/bin/spytial-example
RUN chmod +x /usr/local/bin/spytial-example /usr/local/bin/viz-server

ENV SPYTIAL_NO_OPEN=1
ENV SPYTIAL_PORT=8080

EXPOSE 8080

ENTRYPOINT ["/usr/local/bin/spytial-example"]
CMD ["rbt"]
