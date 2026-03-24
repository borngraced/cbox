# cbox personal sandbox image
# Customize this with your preferred shell, tools, and configuration.
#
# Build:  docker build -t cbox-dev .
# Use:    cbox run --image cbox-dev
# Or set permanently in cbox.toml:
#   [sandbox]
#   image = "cbox-dev"

FROM ubuntu:24.04

# Avoid interactive prompts during package installation
ENV DEBIAN_FRONTEND=noninteractive

# Base development tools
RUN apt-get update && apt-get install -y --no-install-recommends \
    # Shells
    bash \
    zsh \
    fish \
    # Core utilities
    curl \
    wget \
    git \
    vim \
    less \
    jq \
    tree \
    htop \
    # Build essentials
    build-essential \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# --- Uncomment/add sections below as needed ---

# Python
# RUN apt-get update && apt-get install -y python3 python3-pip && \
#     rm -rf /var/lib/apt/lists/*

# Rust
# RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
# ENV PATH="/root/.cargo/bin:${PATH}"

# Go
# RUN curl -fsSL https://go.dev/dl/go1.23.0.linux-amd64.tar.gz | tar -C /usr/local -xz
# ENV PATH="/usr/local/go/bin:${PATH}"

# Claude Code
RUN curl -fsSL https://claude.ai/install.sh | bash
ENV PATH="/root/.local/bin:${PATH}"

# Default shell — change to /usr/bin/fish or /usr/bin/zsh if preferred
ENV SHELL=/bin/bash
