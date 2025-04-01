#!/bin/bash

set -e

# Docker would correctly not follow symlinks outside the current directory,
# but my goal remains that Docker-based builds are reproducible,
# yet the `lib` directory from the root of the reposirory is shared.
# Since the default Cargo-based build would create a symlink,
# this symlink needs to be removed first. If there already is a dir, it's OK to stay.
[ -L code/lib ] && (unlink code/lib && echo 'Symlink of `code/lib` removed.') || echo 'No symlink to remove.'
[ -d code/lib ] && echo 'The `code/lib` dir exists, using it.' || (cp -r ../lib code && echo 'Copied `../lib` into `code/`.')

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

echo "RAW JSON"
curl -s localhost:3000/json

echo "NICE HTML"
curl -s -H "Accept: text/html" localhost:3000/json

curl -s localhost:3000/quit

wait $PID
