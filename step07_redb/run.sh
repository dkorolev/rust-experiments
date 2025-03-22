#!/bin/bash

set -e

docker build -f ../Dockerfile.template . -t demo

docker run --rm --network=bridge -p 3000:3000 -t demo &
PID=$!

while true ; do
  R="$(curl -s localhost:3000/healthz || echo NOPE)"
  if [ "$R" = "OK" ] ; then
    echo "server healthy"
    break
  fi
  sleep 0.5
  echo "server not yet healthy"
done

curl -s localhost:3000

curl -s localhost:3000/json

curl -s localhost:3000/quit

wait $PID
