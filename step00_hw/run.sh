#!/bin/bash

set -e

docker build -f ../Dockerfile.template . -t demo
docker run --rm -t demo
