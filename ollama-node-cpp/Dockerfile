# Multi-stage build for the C++ node
FROM debian:bookworm AS build

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential cmake libssl-dev git ca-certificates && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY . /src

RUN cmake -S /src -B /build \
    -DCMAKE_BUILD_TYPE=Release \
    -DBUILD_TESTS=OFF && \
    cmake --build /build --config Release && \
    cmake --install /build --prefix /usr/local

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    libssl3 libstdc++6 ca-certificates && \
    rm -rf /var/lib/apt/lists/*

ENV OLLAMA_MODELS_DIR=/var/lib/ollama/models
ENV OLLAMA_ROUTER_URL=http://router:8080
ENV OLLAMA_NODE_PORT=11435

WORKDIR /app
COPY --from=build /usr/local /usr/local

EXPOSE 11435
VOLUME ["/var/lib/ollama/models"]

ENTRYPOINT ["/usr/local/bin/ollama-node"]
