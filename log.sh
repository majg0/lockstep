#!/bin/bash

set -e

F=tmp/running_processes.log

P=${1:-server}

if [ -s $F ]; then
    pid=$(awk -v p="$P" '$2==p {print $1}' $F)
    if [ -n "$pid" ]; then
        tail -f "tmp/$P.log"
    else
        echo "No running process '$P'."
    fi
else
    echo "No running processes."
fi

