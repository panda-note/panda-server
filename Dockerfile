# syntax=docker/dockerfile:1

FROM rust:1-bookworm AS builder
WORKDIR /src

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY config ./config
COPY migrations ./migrations
COPY openapi.yaml ./openapi.yaml
COPY proto ./proto

# Newer rustc can ICE on dead_code lint for this workspace; keep builds green.
ENV RUSTFLAGS="-A dead_code"

RUN cargo build --release -p server \
    && strip /src/target/release/panda \
    && mkdir -p /out/data/blobs

# CI extracts just the binary for linux/amd64 and linux/arm64 packages.
FROM scratch AS binary
COPY --from=builder /src/target/release/panda /panda

FROM gcr.io/distroless/cc-debian12:nonroot
WORKDIR /

COPY --from=builder /src/target/release/panda /usr/local/bin/panda
COPY --from=builder --chown=nonroot:nonroot /out/data /data
COPY --chown=nonroot:nonroot config/docker.yml /etc/panda/config.yml

USER nonroot:nonroot
VOLUME ["/data"]
EXPOSE 8787

ENTRYPOINT ["/usr/local/bin/panda"]
CMD ["--config", "/etc/panda/config.yml"]
