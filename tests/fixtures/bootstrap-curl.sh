#!/bin/sh
set -eu
[ "${PARROT_TEST_FAIL:-0}" = 0 ] || exit 22
while [ "$#" -gt 0 ]; do
    if [ "$1" = --output ]; then
        cp "$PARROT_TEST_ARCHIVE" "$2"
        exit 0
    fi
    shift
done
exit 1
