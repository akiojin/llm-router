# ==============================================================================
# Development Environment (no cross-compilation)
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

# Install GitHub CLI
RUN curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg | dd of=/usr/share/keyrings/githubcli-archive-keyring.gpg \
    && chmod go+r /usr/share/keyrings/githubcli-archive-keyring.gpg \
    && echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" | tee /etc/apt/sources.list.d/github-cli.list > /dev/null \
    && apt update \
    && apt install gh -y \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

# NOTE: GitHub CLI extensions are installed at runtime in entrypoint.sh
# (requires authentication which is not available during build)

# Install Rust toolchain
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

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
    @commitlint/cli@latest \
    @commitlint/config-conventional@latest

EXPOSE 8080

WORKDIR /llm-router
# Use bash to invoke entrypoint to avoid exec-bit and CRLF issues on Windows mounts
ENTRYPOINT ["bash", "/llm-router/scripts/entrypoint.sh"]
CMD ["bash"]
