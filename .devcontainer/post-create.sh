#!/usr/bin/env bash

set -euo pipefail

if ! cargo watch --version >/dev/null 2>&1; then
  cargo install cargo-watch --locked
fi

npm install