ARG CUDA=cpu

# CPU build stage
FROM debian:bookworm AS build-cpu
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential cmake libssl-dev git ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# CUDA build stage
FROM nvidia/cuda:12.4.1-devel-ubuntu22.04 AS build-cuda
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential cmake libssl-dev git ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# Select build stage based on CUDA arg (cpu|cuda)
FROM build-${CUDA} AS build
WORKDIR /src
COPY . /src
RUN cmake -S /src -B /build \
    -DCMAKE_BUILD_TYPE=Release \
    -DBUILD_TESTS=OFF \
    -DBUILD_WITH_CUDA=$([ "$CUDA" = "cuda" ] && echo ON || echo OFF) && \
    cmake --build /build --config Release && \
    cmake --install /build --prefix /usr/local && \
    strip /usr/local/bin/llm-node || true

# Runtime images
FROM debian:bookworm-slim AS runtime-cpu
RUN apt-get update && apt-get install -y --no-install-recommends \
    libssl3 libstdc++6 ca-certificates && \
    rm -rf /var/lib/apt/lists/*

FROM nvidia/cuda:12.4.1-runtime-ubuntu22.04 AS runtime-cuda
RUN apt-get update && apt-get install -y --no-install-recommends \
    libssl3 libstdc++6 ca-certificates && \
    rm -rf /var/lib/apt/lists/*

FROM runtime-${CUDA} AS runtime

ENV LLM_MODELS_DIR=/var/lib/runtime/models
ENV LLM_ROUTER_URL=http://router:8080
ENV LLM_NODE_PORT=11435

WORKDIR /app
COPY --from=build /usr/local /usr/local

EXPOSE 11435
VOLUME ["/var/lib/runtime/models"]

ENTRYPOINT ["/usr/local/bin/llm-node"]
