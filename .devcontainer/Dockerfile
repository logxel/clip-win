FROM mcr.microsoft.com/devcontainers/rust:2-1-bookworm

RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        build-essential \
        curl \
        file \
        libasound2-dev \
        libayatana-appindicator3-dev \
        libdbus-1-dev \
        libglib2.0-dev \
        libgtk-3-dev \
        libssl-dev \
        librsvg2-dev \
        libudev-dev \
        libwayland-dev \
        libwebkit2gtk-4.1-dev \
        libxdo-dev \
        pkg-config \
        wget \
        wl-clipboard \
        xclip \
        xvfb \
    && rm -rf /var/lib/apt/lists/*

USER vscode

RUN rustup component add rustfmt clippy