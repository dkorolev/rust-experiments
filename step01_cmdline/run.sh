#!/bin/bash

set -e

docker build -f ../Dockerfile.template . -t demo
docker run --rm -t demo --a 1 --b 2
docker run --rm -t demo --a 3 --b 4
