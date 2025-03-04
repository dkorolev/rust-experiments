#!/bin/bash

set -e

docker build . -t demo
docker run --rm -t demo
