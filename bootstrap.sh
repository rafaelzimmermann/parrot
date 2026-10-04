#!/bin/sh
# Download and build Parrot without Git. All arguments go to install.sh.
# PARROT_REF selects a branch, tag, or commit (default: main).

set -eu

main() {
    for tool in curl tar bash mktemp; do
        command -v "$tool" >/dev/null 2>&1 || {
            printf 'parrot: required command missing: %s\n' "$tool" >&2
            return 1
        }
    done
    for arg do
        if [ "$arg" = --skip-build ]; then
            printf 'parrot: --skip-build requires a local checkout and cannot be used with bootstrap.sh\n' >&2
            return 1
        fi
    done
    ref=${PARROT_REF:-main}
    case "$ref" in
        *[!a-zA-Z0-9._/-]*) printf 'parrot: invalid PARROT_REF\n' >&2; return 1 ;;
    esac
    parrot_tmp=$(mktemp -d "${TMPDIR:-/tmp}/parrot-install.XXXXXXXX")
    trap 'rm -rf -- "$parrot_tmp"' EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM
    printf 'Downloading Parrot source (%s)…\n' "$ref"
    curl --fail --show-error --silent --location --proto '=https' --proto-redir '=https' \
        --retry 3 --connect-timeout 20 --max-time 300 \
        "https://codeload.github.com/rafaelzimmermann/parrot/tar.gz/$ref" \
        --output "$parrot_tmp/source.tar.gz"
    mkdir "$parrot_tmp/source"
    tar -xzf "$parrot_tmp/source.tar.gz" --strip-components=1 -C "$parrot_tmp/source"
    test -f "$parrot_tmp/source/Cargo.toml" && test -f "$parrot_tmp/source/install.sh" || {
        printf 'parrot: downloaded archive is missing project files\n' >&2
        return 1
    }
    # The downloaded script, not the incoming shell pipe, supplies installer code.
    bash "$parrot_tmp/source/install.sh" "$@" </dev/null
}

main "$@"
