#!/usr/bin/env bash

DIR="$(cd "$(dirname "$0")" && pwd)"

cd $DIR

mkdir ../openapi
cargo run --bin docs >../openapi/index.html
