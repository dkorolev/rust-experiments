#!/bin/bash
set -e

echo "Building Maroon JSON executor..."
cd code
cargo build --release

echo -e "\nCopying bytecode file..."
cp ../../../factorial_bytecode.json .

echo -e "\nExecuting factorial(5) from JSON bytecode:"
cargo run --release factorial_bytecode.json 5