#!/bin/bash

set -e

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
curl -s localhost:3000/json
echo
curl -s localhost:3000/json
echo
curl -s localhost:3000/string
echo
curl -s localhost:3000/string
echo

curl -s http://localhost:3000/journal | jq .
echo

curl -s -d '{"id": "test", "a": 2, "b": 2, "c": 5}' http://localhost:3000/sums
curl -s -d '{"id": "test", "a": 2, "b": 3, "c": 5}' http://localhost:3000/sums

sleep 1
TS=$(date +%s)
echo "TS: $TS"

curl -s -d '{"id": "test'$TS'-a", "a": 2, "b": 2, "c": 5}' http://localhost:3000/sums
curl -s -d '{"id": "test'$TS'-b", "a": 2, "b": 3, "c": 5}' http://localhost:3000/sums

curl -s http://localhost:3000/journal | jq .
echo
