FROM rust:1.100-bookworm AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY rtools-domain ./rtools-domain
COPY rtools-sync-proto ./rtools-sync-proto
COPY rtools-syncd ./rtools-syncd
RUN sed -i '/"rtools-engine",/d' Cargo.toml \
    && cargo build --release -p rtools-syncd \
    && cp target/release/rtools-syncd /usr/local/bin/rtools-syncd

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/bin/rtools-syncd /usr/local/bin/rtools-syncd
VOLUME ["/data"]
EXPOSE 18092
ENTRYPOINT ["/usr/local/bin/rtools-syncd"]
CMD ["--data-dir", "/data", "serve", "--bind", "0.0.0.0:18092"]

