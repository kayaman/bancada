#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_dir="$(cd -- "$script_dir/../../../.." && pwd)"

exec cargo run --quiet --manifest-path "$repo_dir/Cargo.toml" \
  -p bancada-core --bin bancada-circuit -- "$@"
