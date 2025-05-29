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

# Build and run the example directly
docker run --rm -v $(pwd)/code:/code --workdir /code rust:alpine sh -c "
  apk add --no-cache musl-dev && \
  cargo run --example tasks_dag
"