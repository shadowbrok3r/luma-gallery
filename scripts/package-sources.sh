#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p assets/source
tar --exclude='.git' --exclude='native/libs' --exclude='java/com' \
    --exclude='assets/source' --exclude='__pycache__' \
    -cJf assets/source/luma-native-source.tar.xz \
    Cargo.toml Cargo.lock LICENSE README.md THIRD_PARTY.md VERIFICATION.md src java/app native scripts icon assets/icons
echo 'Packaged application, build recipes, native dependencies and license notices.'
