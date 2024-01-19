#/bin/bash

F=tmp/running_processes.log

while [ -s $F ]; do
  while IFS= read -r line; do
    pid=$(echo $line | awk '{print $1}')
    name=$(echo $line | awk '{print $2}')

    if ! ps -p $pid > /dev/null; then
      # NOTE: \a is the bell character, which marks the terminal and makes a sound
      echo "\aProcess $name with PID $pid has died."
      sed -i '' "/^$pid /d" $F
    fi
  done < "$F"

  sleep 1
done
