#!/bin/bash

set -e

docker build . -t demo

echo '```mermaid' >.mermaid.md
docker run --rm -t demo | tee >>.mermaid.md
echo '```' >>.mermaid.md

# TODO(dkorolev): Fail with a nicer message, and add a git hook.
diff mermaid.md .mermaid.md || echo 'Your `mermaid.md` file is not up to date.'
