#!/bin/sh

set -e

cargo fmt --check
cargo clippy
cargo test
