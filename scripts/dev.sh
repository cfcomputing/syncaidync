#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
export SYNCAIDYNC_WEB_DIR="$PWD/web"
export SYNCAIDYNC_DEV_SHELL=1
exec cargo run -- serve "$@"
