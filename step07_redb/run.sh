#!/bin/bash

set -e

# Docker would correctly not follow symlinks outside the current directory,
# but my goal remains that Docker-based builds are reproducible,
# yet the `lib` directory from the root of the reposirory is shared.
# Since the default Cargo-based build would create a symlink,
# this symlink needs to be removed first. If there already is a dir, it's OK to stay.
[ -L code/src/lib ] && (unlink code/src/lib && echo 'Symlink of `code/src/lib` removed.') || echo 'No `code/src/lib` symlink to remove.'
[ -d code/src/lib ] && echo 'The `code/src/lib` dir exists, using it.' || (cp -r ../lib code/src && echo 'Copied `../lib` into `code/src`.')
[ -L code/templates ] && (unlink code/templates && echo 'Symlink of `code/templates` removed.') || echo 'No `code/templates` symlink to remove.'
[ -d code/templates ] && echo 'The `code/templates` dir exists, using it.' || (cp -r ../lib/templates code/ && echo 'Copied `../lib/templates` into `code/templates`.')

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
  curl -s localhost:3000/json
  echo
  curl -s localhost:3000/json
  echo

  curl -s localhost:3000/quit
  wait $PID
  echo
done

echo "All runs done."
