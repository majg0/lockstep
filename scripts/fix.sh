#!/bin/sh

set -e

cargo fmt
cargo clippy --allow-dirty --allow-staged --fix
