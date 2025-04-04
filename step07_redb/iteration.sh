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
