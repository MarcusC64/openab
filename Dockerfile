# --- Build stage ---
FROM rust:1-bookworm AS builder

# Build openab
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && cargo build --release && rm -rf src
COPY src/ src/
RUN touch src/main.rs && cargo build --release

# Build openab-gateway
WORKDIR /build/gateway
COPY gateway/Cargo.toml gateway/Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && cargo build --release && rm -rf src
COPY gateway/src/ src/
RUN touch src/main.rs && cargo build --release

# --- Runtime stage ---
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl procps ripgrep tini unzip ffmpeg && rm -rf /var/lib/apt/lists/*

# Install kiro-cli (auto-detect arch, copy binary directly)
ARG KIRO_CLI_VERSION=2.2.0
RUN ARCH=$(dpkg --print-architecture) && \
    if [ "$ARCH" = "arm64" ]; then URL="https://prod.download.cli.kiro.dev/stable/${KIRO_CLI_VERSION}/kirocli-aarch64-linux.zip"; \
    else URL="https://prod.download.cli.kiro.dev/stable/${KIRO_CLI_VERSION}/kirocli-x86_64-linux.zip"; fi && \
    curl --proto '=https' --tlsv1.2 -sSf --retry 3 --retry-delay 5 "$URL" -o /tmp/kirocli.zip && \
    unzip /tmp/kirocli.zip -d /tmp && \
    cp /tmp/kirocli/bin/* /usr/local/bin/ && \
    chmod +x /usr/local/bin/kiro-cli* && \
    rm -rf /tmp/kirocli /tmp/kirocli.zip

# Install gh CLI
RUN curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg \
      -o /usr/share/keyrings/githubcli-archive-keyring.gpg && \
    echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" \
      > /etc/apt/sources.list.d/github-cli.list && \
    apt-get update && apt-get install -y --no-install-recommends gh && \
    rm -rf /var/lib/apt/lists/*

RUN useradd -m -s /bin/bash -u 1000 agent
RUN mkdir -p /home/agent/.local/share/kiro-cli /home/agent/.kiro && \
    chown -R agent:agent /home/agent
RUN mkdir -p /etc/openab && chown agent:agent /etc/openab
ENV HOME=/home/agent
WORKDIR /home/agent

COPY --from=builder --chown=agent:agent /build/target/release/openab /usr/local/bin/openab
COPY --from=builder --chown=agent:agent /build/gateway/target/release/openab-gateway /usr/local/bin/openab-gateway
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh

USER agent
HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
  CMD pgrep -x openab || exit 1
ENTRYPOINT ["tini", "--"]
CMD ["/usr/local/bin/docker-entrypoint.sh"]
