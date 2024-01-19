#/bin/bash

F=tmp/running_processes.log

if [ -s $F ]; then
    awk '{print $1}' $F | xargs ps -p
else
    echo "No running processes."
fi
