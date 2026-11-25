#!/bin/bash
# Build a release binary inside Ubuntu 20.04 (Dockerfile.compat), for systems
# with an older glibc than this machine has. The binary is written to
# target/vltview.
set -euo pipefail

# Dockerfile.compat and the build context are the repository, wherever the
# script is called from
cd "$(dirname "$0")"

docker build -f Dockerfile.compat -t vltview-builder .

# Create target/ first and copy as the current user, so that neither the
# directory nor the binary ends up owned by root
mkdir -p target
docker run --rm --user "$(id -u):$(id -g)" -v "$(pwd)/target:/output" vltview-builder

echo "Compatible binary created in: target/vltview"
