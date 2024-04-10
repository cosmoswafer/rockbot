#!/bin/bash

PYTHON_VENV="venv" # No trailing slash
PYTHON_SCRIPT="rock.py"

function start_bg_python
{
    # $1 : Python script
    # $2 : Python virtual environment

    python_script="$1"
    if [ ! -f "$python_script" ]
    then
        echo "Python script not found: $python_script"
        exit 1
    else
        echo "Python script: $python_script"
    fi

    python_interpreter=$(which python3)
    if [ -d "$2" ] && [ -x "$2/bin/python" ]
    then
        python_interpreter="$2/bin/python"
    fi
    echo "Python interpreter: $python_interpreter"

    total_retry_count=5
    retry_count=$total_retry_count
    while [ $retry_count -gt 0 ]
    do
    retry_count=$((retry_count - 1))
    timestamp_start=$(date +%Y%m%d%H%M%S)

    # Actually run the script within virtual environment
    "$python_interpreter" "$1"
    
    timestamp_now=$(date +%Y%m%d%H%M%S)
    # Compare the timestamp if it run longer than 5 minutes
    if [ $((timestamp_now - timestamp_start)) -gt 300 ]
    then
        retry_count=$total_retry_count
        echo "Reset the retry count"
    else
        echo "Python script crashed, retry ($retry_count/$total_retry_count) ..."
    fi
    if [ -f "stop_$PYTHON_SCRIPT" ]
    then
        echo "Stop Python script $1"
        retry_count=0
        break
    fi

    sleep 5
    done
}
export -f start_bg_python

[ -f "stop_$PYTHON_SCRIPT" ] && rm -vf "stop_$PYTHON_SCRIPT"
[ -f nohup.out ] && rm -vf nohup.out
nohup bash -c "start_bg_python $PYTHON_SCRIPT $PYTHON_VENV" &
sleep 3
ps -ef | grep python | grep "$PYTHON_SCRIPT"

echo "To kill the background process: touch stop_$PYTHON_SCRIPT && kill <pid>"
