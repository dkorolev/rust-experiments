#!/bin/bash
for d in $(find . -type f -iname 'Cargo.toml' | xargs dirname | sort | uniq) ; do (cd $d ; cargo fmt) ; done
