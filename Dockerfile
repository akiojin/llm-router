# ==============================================================================
# Stage 1: osxcross Build Environment
# ==============================================================================
FROM ubuntu:22.04 AS osxcross-builder

ARG SDK_VERSION=14.2
ARG OSX_VERSION_MIN=11.0

# Install build dependencies for osxcross
RUN apt-get update && apt-get install -y \
    build-essential \
    clang \
    cmake \
    git \
    curl \
    wget \
    libxml2-dev \
    libssl-dev \
    libbz2-dev \
    libz-dev \
    liblzma-dev \
    llvm-dev \
    uuid-dev \
    patch \
    python3 \
    python-is-python3 \
    xz-utils \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

# Clone osxcross
WORKDIR /opt
RUN git clone https://github.com/tpoechtrager/osxcross.git

# Copy macOS SDK (must be provided by user)
COPY .sdk/MacOSX${SDK_VERSION}.sdk.tar.xz /opt/osxcross/tarballs/

# Build osxcross
WORKDIR /opt/osxcross
RUN UNATTENDED=yes OSX_VERSION_MIN=${OSX_VERSION_MIN} ./build.sh

# ==============================================================================
# Stage 2: Development Environment with osxcross
# ==============================================================================
FROM node:22-bookworm

RUN apt-get update && apt-get install -y \
    jq \
    ripgrep \
    curl \
    dos2unix \
    ca-certificates \
    gnupg \
    vim \
    clang \
    libxml2 \
    libssl3 \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

# Copy osxcross toolchain from builder stage
COPY --from=osxcross-builder /opt/osxcross/target /opt/osxcross/target

# Add osxcross to PATH
ENV PATH="/opt/osxcross/target/bin:${PATH}"
ENV LD_LIBRARY_PATH="/opt/osxcross/target/lib:${LD_LIBRARY_PATH}"

# Install GitHub CLI
RUN curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg | dd of=/usr/share/keyrings/githubcli-archive-keyring.gpg \
    && chmod go+r /usr/share/keyrings/githubcli-archive-keyring.gpg \
    && echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" | tee /etc/apt/sources.list.d/github-cli.list > /dev/null \
    && apt update \
    && apt install gh -y \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

# Install .NET 9 SDK (for C# LSP build) via official install script
ENV DOTNET_ROOT=/usr/share/dotnet
ENV PATH="${DOTNET_ROOT}:${PATH}"
RUN set -eux; \
    curl -fsSL -o /tmp/dotnet-install.sh https://dot.net/v1/dotnet-install.sh; \
    chmod +x /tmp/dotnet-install.sh; \
    /tmp/dotnet-install.sh --channel 9.0 --install-dir "$DOTNET_ROOT"; \
    ln -sf "$DOTNET_ROOT/dotnet" /usr/bin/dotnet; \
    dotnet --info

# Install Rust toolchain
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

# Add macOS targets for cross-compilation
RUN rustup target add x86_64-apple-darwin aarch64-apple-darwin

# Install uv/uvx
RUN curl -fsSL https://astral.sh/uv/install.sh | bash

RUN npm i -g pnpm@latest

# Setup pnpm global bin directory manually
ENV PNPM_HOME="/root/.local/share/pnpm"
ENV PATH="$PNPM_HOME:$PATH"

RUN mkdir -p "$PNPM_HOME" && \
    pnpm config set global-bin-dir "$PNPM_HOME" && \
    echo 'export PNPM_HOME="/root/.local/share/pnpm"' >> /root/.bashrc && \
    echo 'export PATH="$PNPM_HOME:$PATH"' >> /root/.bashrc

RUN npm i -g \
    npm@latest \
    pnpm@latest \
    bun@latest \
    typescript@latest \
    eslint@latest \
    prettier@latest \
    @commitlint/cli@latest \
    @commitlint/config-conventional@latest

# Setup pnpm global bin directory manually
ENV PNPM_HOME="/root/.local/share/pnpm"
ENV PATH="$PNPM_HOME:$PATH"

RUN mkdir -p "$PNPM_HOME" && \
    pnpm config set global-bin-dir "$PNPM_HOME" && \
    echo 'export PNPM_HOME="/root/.local/share/pnpm"' >> /root/.bashrc && \
    echo 'export PATH="$PNPM_HOME:$PATH"' >> /root/.bashrc

EXPOSE 8080

WORKDIR /ollama-coordinator
# Use bash to invoke entrypoint to avoid exec-bit and CRLF issues on Windows mounts
ENTRYPOINT ["bash", "/ollama-coordinator/scripts/entrypoint.sh"]
CMD ["bash"]
