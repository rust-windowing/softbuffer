#!/bin/sh

set -eu

echo "::warning::pinning dev-deps versions for CI tests"

set -x
sed -ri 's/(criterion = .+)$/\1\nhalf = "=2.2.1"/' Cargo.toml
