#!/bin/bash

STDERR_FILE=$(mktemp)

RUST_LOG=chatserver=trace cargo run -- client "" 2> >(tee $STDERR_FILE)
cat $STDERR_FILE

rm $STDERR_FILE
