# syntax=docker/dockerfile:1.7

ARG RUST_VERSION=1.96.0
ARG REVISION=unknown

FROM rust:${RUST_VERSION}-slim-bookworm AS build-base

ARG TARGETARCH

ENV CARGO_NET_GIT_FETCH_WITH_CLI=true
WORKDIR /workspace

RUN apt-get update && \
    apt-get install -y --no-install-recommends build-essential ca-certificates git pkg-config && \
    rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src src

FROM build-base AS build

RUN --mount=type=cache,id=homelab-mcp-${TARGETARCH}-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=homelab-mcp-${TARGETARCH}-cargo-git,target=/usr/local/cargo/git \
    --mount=type=cache,id=homelab-mcp-${TARGETARCH}-cargo-target,target=/workspace/target \
    --mount=type=secret,id=gitconfig,target=/root/.gitconfig,required=false \
    --mount=type=secret,id=git-credentials,target=/root/.git-credentials,required=false \
    cargo build --locked --release && \
    cp /workspace/target/release/homelab-mcp /usr/local/bin/homelab-mcp

FROM debian:bookworm-slim AS kubectl

ARG TARGETARCH
ARG KUBECTL_VERSION=v1.33.5

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates curl && \
    rm -rf /var/lib/apt/lists/* && \
    case "${TARGETARCH}" in \
      amd64) checksum=6a12d6c39e4a611a3687ee24d8c733961bb4bae1ae975f5204400c0a6930c6fc ;; \
      arm64) checksum=6db7c5d846c3b3ddfd39f3137a93fe96af3938860eefdbf2429805ee1656e381 ;; \
      *) echo "unsupported TARGETARCH: ${TARGETARCH}" >&2; exit 1 ;; \
    esac && \
    curl --fail --location --proto '=https' --tlsv1.2 \
      "https://dl.k8s.io/release/${KUBECTL_VERSION}/bin/linux/${TARGETARCH}/kubectl" \
      --output /usr/local/bin/kubectl && \
    printf '%s  %s\n' "${checksum}" /usr/local/bin/kubectl | sha256sum --check --strict && \
    chmod 0755 /usr/local/bin/kubectl

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
COPY --from=kubectl /usr/local/bin/kubectl /usr/local/bin/kubectl

EXPOSE 14333
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/homelab-mcp"]
