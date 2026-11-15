# syntax=docker/dockerfile:1
ARG RUST_VERSION=1.99

FROM rust:${RUST_VERSION}-slim-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked \
    && install -m 0755 target/release/bon /bon

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /bon /usr/local/bin/bon
USER 10001:10001
ENV RUST_LOG="info,bon=debug"
EXPOSE 8080

ENTRYPOINT ["/usr/local/bin/bon"]
