# syntax=docker/dockerfile:1.7

ARG RUST_VERSION=1.96.0
ARG REVISION=unknown

FROM rust:${RUST_VERSION}-slim-bookworm AS build

ARG TARGETARCH

ENV CARGO_NET_GIT_FETCH_WITH_CLI=true
WORKDIR /workspace

RUN apt-get update && \
    apt-get install -y --no-install-recommends build-essential ca-certificates git pkg-config && \
    rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src src

RUN --mount=type=cache,id=homelab-mcp-${TARGETARCH}-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=homelab-mcp-${TARGETARCH}-cargo-git,target=/usr/local/cargo/git \
    --mount=type=cache,id=homelab-mcp-${TARGETARCH}-cargo-target,target=/workspace/target \
    --mount=type=secret,id=gitconfig,target=/root/.gitconfig,required=false \
    --mount=type=secret,id=git-credentials,target=/root/.git-credentials,required=false \
    cargo build --locked --release && \
    cp /workspace/target/release/homelab-mcp /usr/local/bin/homelab-mcp

FROM debian:bookworm-slim AS runtime

ARG REVISION

LABEL org.opencontainers.image.source="https://git.holdenitdown.net/rfhold/homelab-mcp" \
      org.opencontainers.image.revision="${REVISION}"

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/* && \
    install -d -o 65532 -g 65532 /data

ENV HOME=/data
WORKDIR /data

COPY --from=build /usr/local/bin/homelab-mcp /usr/local/bin/homelab-mcp

EXPOSE 14333
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/homelab-mcp"]
