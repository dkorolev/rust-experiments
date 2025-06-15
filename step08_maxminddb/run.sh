#!/bin/bash

set -e

cat ../Dockerfile.template | grep -B 999999 '^# PLACEHOLDER_BUILD$' >Dockerfile

echo 'RUN apk add wget' >>Dockerfile
echo 'RUN wget https://github.com/P3TERX/GeoLite.mmdb/raw/download/GeoLite2-Country.mmdb' >>Dockerfile

cat ../Dockerfile.template | grep -A 999999 '^# PLACEHOLDER_BUILD$' | grep -B 999999 '^# PLACEHOLDER_RUN$' >>Dockerfile

echo 'COPY --from=build /GeoLite2-Country.mmdb /' >>Dockerfile

cat ../Dockerfile.template | grep -A 999999 '^# PLACEHOLDER_RUN$' ../Dockerfile.template >>Dockerfile

docker build . -t demo
docker run --rm -t demo
