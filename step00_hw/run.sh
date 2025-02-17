#!/bin/bash

set -e

docker build . -t demo
docker run -t demo
