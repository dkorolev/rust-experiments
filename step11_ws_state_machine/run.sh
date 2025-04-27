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

echo '`npm i ws`.'
npm i ws
echo '`npm i ws`: success.'

docker build -f ../Dockerfile.template . -t demo

function cleanup {
  echo -n "trap: stopping the docker container "
  docker stop -t 0 rust_experiments_docker_container
  echo "docker stop done"
}
trap cleanup EXIT

docker run --rm --name rust_experiments_docker_container --network=bridge -p 3000:3000 -t demo &
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

WS_NODEJS_CODE="const WS = require('ws'); (new WS('ws://localhost:3000/test_ws')).on('message', buf => console.log(buf.toString()));"

S="$(node -e "$WS_NODEJS_CODE" | head -n 1)"
G="magic"

if [ "$S" != "$G" ] ; then
  echo "TEST FAILED, expected '$G', seeing '$S'."
  exit 1
fi

curl -s localhost:3000/quit

trap - EXIT

wait $PID
