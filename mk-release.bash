#! /usr/bin/env bash

set -euo pipefail

pkg_id="$(cargo pkgid)"
tag=${pkg_id##*#}

cargo test
cargo build --release --target=x86_64-unknown-linux-musl
strip target/x86_64-unknown-linux-musl/release/dt

cargo build --release --target=x86_64-pc-windows-gnu

gh release create "$tag"\
  --draft 
  # --notes-file release_notes.md 
  
gh release upload "$tag" "target/x86_64-unknown-linux-musl/release/dt#64-bit linux musl"
gh release upload "$tag" "target/x86_64-pc-windows-gnu/release/dt.exe#64-bit windows"
  
