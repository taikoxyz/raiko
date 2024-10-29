#!/usr/bin/env bash

DIR="$(cd "$(dirname "$0")" && pwd)"

TASKDB=${TASKDB:-raiko-tasks/in-memory}

cd $DIR

mkdir ../openapi
cargo run -F ${TASKDB} --bin docs >../openapi/index.html
