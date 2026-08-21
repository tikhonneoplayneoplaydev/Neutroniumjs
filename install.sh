#!/usr/bin/env sh
set -eu
# Builds an optimized binary and installs it into Cargo's user bin directory.
command -v cargo >/dev/null 2>&1 || { echo 'Install Rust first: https://rustup.rs' >&2; exit 1; }
cargo install --path .
printf '\nInstalled neut. Ensure ~/.cargo/bin is in PATH:\n  export PATH="$HOME/.cargo/bin:$PATH"\n'
