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
COPY migrations migrations

FROM build-base AS build

RUN --mount=type=cache,id=homelab-mcp-${TARGETARCH}-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=homelab-mcp-${TARGETARCH}-cargo-git,target=/usr/local/cargo/git \
    --mount=type=cache,id=homelab-mcp-${TARGETARCH}-cargo-target,target=/workspace/target \
    --mount=type=secret,id=gitconfig,target=/root/.gitconfig,required=false \
    --mount=type=secret,id=git-credentials,target=/root/.git-credentials,required=false \
    cargo build --locked --release && \
    cp /workspace/target/release/homelab-mcp /usr/local/bin/homelab-mcp

FROM python:3.13-slim-bookworm AS deploy-runtime

ARG TARGETARCH
ARG UV_VERSION=0.11.15

WORKDIR /opt/homelab-mcp

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates curl && \
    rm -rf /var/lib/apt/lists/* && \
    case "${TARGETARCH}" in \
      amd64) uv_url=https://files.pythonhosted.org/packages/d3/16/fe392d618af6b00c064b3e718d585dcf791546a77c5123a5bec07ce53a0a/uv-${UV_VERSION}-py3-none-manylinux_2_17_x86_64.manylinux2014_x86_64.whl; checksum=98edf1bdaf82447014852051d93e3ee95012509c567bf057fd117e6bdbd9a807 ;; \
      arm64) uv_url=https://files.pythonhosted.org/packages/af/50/4bc8a148274feabee2d9c9f1fa15009e10c0228dfe57981ee3ea2ef1d481/uv-${UV_VERSION}-py3-none-manylinux_2_17_aarch64.manylinux2014_aarch64.musllinux_1_1_aarch64.whl; checksum=c0cf52cd6d50bb9e05e2d968f45f80761107e4cbc8d4a26d9758f9d8274aaec1 ;; \
      *) echo "unsupported TARGETARCH: ${TARGETARCH}" >&2; exit 1 ;; \
    esac && \
    curl --fail --location --proto '=https' --tlsv1.2 "${uv_url}" --output /tmp/uv.whl && \
    printf '%s  %s\n' "${checksum}" /tmp/uv.whl | sha256sum --check --strict && \
    python -m zipfile --extract /tmp/uv.whl /tmp/uv-wheel && \
    install -m 0755 /tmp/uv-wheel/uv-${UV_VERSION}.data/scripts/uv /usr/local/bin/uv && \
    rm -rf /tmp/uv.whl /tmp/uv-wheel

COPY pyproject.toml uv.lock ./
RUN --mount=type=cache,id=homelab-mcp-${TARGETARCH}-uv,target=/root/.cache/uv \
    uv sync --locked --no-dev --no-install-project
COPY deploys deploys

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

FROM python:3.13-slim-bookworm AS runtime

ARG REVISION

LABEL org.opencontainers.image.source="https://git.holdenitdown.net/rfhold/homelab-mcp" \
      org.opencontainers.image.revision="${REVISION}"

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates openssh-client && \
    rm -rf /var/lib/apt/lists/* && \
    groupadd --gid 65532 homelab-mcp && \
    useradd --uid 65532 --gid 65532 --home-dir /data --no-create-home --shell /usr/sbin/nologin homelab-mcp && \
    install -d -o 65532 -g 65532 /data

ENV HOME=/data \
    PATH=/opt/homelab-mcp/.venv/bin:/usr/local/bin:/usr/bin:/bin \
    UV_OFFLINE=1 \
    UV_NO_CACHE=1 \
    UV_NO_SYNC=1 \
    UV_PROJECT_ENVIRONMENT=/opt/homelab-mcp/.venv \
    PYTHONDONTWRITEBYTECODE=1
WORKDIR /data

COPY --from=build /usr/local/bin/homelab-mcp /usr/local/bin/homelab-mcp
COPY --from=kubectl /usr/local/bin/kubectl /usr/local/bin/kubectl
COPY --from=deploy-runtime /usr/local/bin/uv /usr/local/bin/uv
COPY --from=deploy-runtime /opt/homelab-mcp /opt/homelab-mcp

EXPOSE 14333
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/homelab-mcp"]
