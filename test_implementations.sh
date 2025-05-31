#!/bin/bash
set -e

echo "Building implementations..."
cd step13_maroon_json && docker build -t step13_maroon -f ../Dockerfile.template . >/dev/null 2>&1
cd ../cpp_validator && docker build -t cpp-validator . >/dev/null 2>&1
cd ..

echo -e "\n=== Running Rust implementation ==="
docker run -v $(pwd)/factorial_bytecode.json:/factorial_bytecode.json step13_maroon /factorial_bytecode.json 5 2>&1

echo -e "\n=== Running C++ implementation ==="
docker run -v $(pwd)/factorial_bytecode.json:/factorial_bytecode.json cpp-validator /factorial_bytecode.json 5 2>&1