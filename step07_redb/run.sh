#!/bin/bash

set -e

docker build -f ../Dockerfile.template . -t demo

mkdir -p .db

N=3
for i in $(seq 1 $N) ; do
  echo "Run $i of $N."

  docker run --rm -v ./.db/:/.db/ --network=bridge -p 3000:3000 -t demo &
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
  echo

  curl -s localhost:3000/quit
  wait $PID
  echo
done

echo "All runs done."
