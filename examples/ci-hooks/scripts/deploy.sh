#!/usr/bin/env bash
# Placeholder deploy script
set -euo pipefail

TARGET="${1:?Usage: deploy.sh <target> [version]}"
VERSION="${2:-dev}"

echo "Deploying version ${VERSION} to ${TARGET}..."
echo "Done."
