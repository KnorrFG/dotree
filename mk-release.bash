#! /usr/bin/env bash

set -euo pipefail

pkg_id="$(cargo pkgid)"
tag=${pkg_id##*#}

cargo test
cargo build --release --target=x86_64-unknown-linux-musl
cargo build --release --target=x86_64-pc-windows-gnu

gh release create "$tag"\
  --draft \
  --notes-file release_notes.md 
  
gh release upload "$tag" target/x86_64-unknown-linux-musl/release/dt \# linux musl binary
gh release upload "$tag" target/x86_64-pc-windows-gnu/release/dt.exe \# windows binary
  
