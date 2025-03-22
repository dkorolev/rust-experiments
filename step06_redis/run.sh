#!/bin/bash

set -e

docker pull redis

docker build -f ../Dockerfile.template . -t demo

docker stop redis-for-rust >/dev/null 2>&1 || true
docker rm redis-for-rust >/dev/null 2>&1 || true

docker run --rm --name redis-for-rust -p 6379:6379 -d redis

echo 'Waiting for Redis ...'

for i in $(seq 20); do
  if docker run --add-host=host.docker.internal:host-gateway --rm -t --network bridge demo --mode check --redis redis://host.docker.internal ; then
    echo 'Redis is up.'
    break
  else
    echo 'Need to wait more.'
    sleep 0.5
  fi
done

echo 'Running the test.'
docker run --add-host=host.docker.internal:host-gateway --rm -t demo --mode test --redis redis://host.docker.internal
echo 'Test run successfully.'

docker stop redis-for-rust
echo 'Redis stopped.'
