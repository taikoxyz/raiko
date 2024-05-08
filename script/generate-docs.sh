#!/usr/bin/env bash

DIR="$(cd "$(dirname "$0")" && pwd)"

cd $DIR

cargo run --bin docs >../index.html
