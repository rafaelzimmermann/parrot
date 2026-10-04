#!/bin/sh
set -eu
[ "${PARROT_TEST_INSTALL_FAIL:-0}" = 0 ] || exit 7
printf '%s\n' "$@" > "$PARROT_TEST_OUTPUT"
