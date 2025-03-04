#!/bin/bash

set -e

docker build -f ../Dockerfile.template . -t demo

echo '```mermaid' >.mermaid.md
docker run --rm -t demo | tee >>.mermaid.md
echo '```' >>.mermaid.md

# TODO(dkorolev): Fail with a nicer message, and add a git hook.
diff -w mermaid.md .mermaid.md && echo "Passed!" || (echo 'Your `mermaid.md` file is not up to date.' && exit 1)
