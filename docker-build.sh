#!/bin/bash
# Build using Docker for compatibility with older systems

docker build -f /tmp/Dockerfile.compat -t vltview-builder .
docker run --rm -v $(pwd)/target:/output vltview-builder

echo "Compatible binary created in: target/vltview"
