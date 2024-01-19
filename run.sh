#/bin/bash

set -e

F=tmp/running_processes.log

N=${1:-2}

mkdir -p tmp

cargo clippy

RUST_BACKTRACE=1 cargo run -p server > tmp/server.log 2>&1 &
echo "$! server" >> $F

for I in $(seq 1 $N)
do
  RUST_BACKTRACE=1 cargo run -p client -- -p 440$I > tmp/$I.log 2>&1 &
  echo "$! $I" >> $F
done

echo Spawned:
cat $F
