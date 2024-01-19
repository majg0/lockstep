#!/bin/bash

F=tmp/running_processes.log

P=$1

if [ -s $F ]; then
  if [ $P ]; then
    pid=$(awk -v p="$P" '$2==p {print $1}' $F)
    if [ -n "$pid" ]; then
        kill $pid
    else
        echo "No running process '$P'."
    fi
  else
    awk '{print $1}' $F | xargs kill
    echo "Killed $(wc -l $F | awk '{print $1}') processes."
    rm $F
  fi
fi
