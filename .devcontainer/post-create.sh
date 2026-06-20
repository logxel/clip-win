#!/usr/bin/env bash
set -euo pipefail

sudo mkdir -p /tmp/runtime-vscode
sudo chown vscode:vscode /tmp/runtime-vscode
sudo chmod 700 /tmp/runtime-vscode

if ! cargo watch --version >/dev/null 2>&1; then
  cargo install cargo-watch --locked
fi

npm install