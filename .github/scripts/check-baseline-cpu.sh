#!/usr/bin/env bash
# Launches a Linux x86_64 Trail binary on an emulated baseline CPU, and fails if
# it dies.
#
# `--version` proves nothing here: it exits before Lua starts, and a binary
# whose C code targets newer CPUs reports its version happily and then crashes
# with "Illegal instruction" on the first real launch.
#
# Runs inside Arch because qemu runs the guest's whole process, dynamic loader
# included, and Ubuntu's own loader is built for newer CPUs and would crash
# first. The `true` control tells that failure apart from Trail's own.
#
# Usage: check-baseline-cpu.sh <path to trail binary>

set -euo pipefail

binary="$(realpath "$1")"

docker run --rm -v "$binary:/trail:ro" archlinux:latest bash -c '
    pacman -Syu --noconfirm --quiet qemu-user > /dev/null
    export TERM=xterm-256color
    mkdir -p /work && printf "hello\n" > /work/a.txt && cd /work

    if ! qemu-x86_64 -cpu qemu64 /usr/bin/true; then
        echo "::error::control failed: the emulated CPU cannot run the system itself"
        exit 2
    fi

    # script gives Trail the terminal it needs; q quits once it has started.
    code=0
    (sleep 15; printf q) | script -qefc "qemu-x86_64 -cpu qemu64 /trail" /tmp/out > /dev/null || code=$?
    if [ "$code" -ne 0 ]; then
        echo "::error::Trail exited with $code on a baseline x86_64 CPU (132 is Illegal instruction)"
        exit 1
    fi
    echo "Trail launched and quit cleanly on a baseline x86_64 CPU"
'
