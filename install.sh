#!/usr/bin/env bash
#
# hypr-speak installer — dependency check, keybind conflict detection,
# window rules, release build, binary install.
#
# Usage: ./install.sh [OPTIONS]
#   --dry-run          Show what would happen, change nothing
#   --key "MODS, KEY"  Keybind to install            (default: "ALT, Escape")
#   --conf PATH        Hyprland config to edit       (default: ~/.config/hypr/hyprland.conf)
#   --no-bind          Don't touch binds
#   --no-rules         Don't add window rules
#   --bin-dir DIR      Install binary here           (default: ~/.local/bin)
#   --skip-build       Don't build (use existing target/release/hypr-speak)
#   -h | --help        This help

set -euo pipefail

DEFAULT_KEY="ALT, Escape"
KEY="$DEFAULT_KEY"
CONF="${XDG_CONFIG_HOME:-$HOME/.config}/hypr/hyprland.conf"
BIN_DIR="$HOME/.local/bin"
DRY_RUN=0 NO_BIND=0 NO_RULES=0 SKIP_BUILD=0
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_NAME="hypr-speak"
APP_ID="hypr-speak"

log()  { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mWARNING:\033[0m %s\n' "$*"; }
die()  { printf '\033[1;31mERROR:\033[0m %s\n' "$*" >&2; exit 1; }

while [[ $# -gt 0 ]]; do
    case "$1" in
        --dry-run)   DRY_RUN=1 ;;
        --key)       KEY="${2:?--key needs a value}"; shift ;;
        --conf)      CONF="${2:?--conf needs a value}"; shift ;;
        --bin-dir)   BIN_DIR="${2:?--bin-dir needs a value}"; shift ;;
        --no-bind)   NO_BIND=1 ;;
        --no-rules)  NO_RULES=1 ;;
        --skip-build) SKIP_BUILD=1 ;;
        -h|--help)   grep '^#' "$0" | sed 's/^# \{0,1\}//g' | head -14; exit 0 ;;
        *)           die "unknown option: $1 (see --help)" ;;
    esac
    shift
done

# --- 1. dependency check ----------------------------------------------------
log "checking dependencies"
for tool in cargo rustc cc pkg-config; do
    command -v "$tool" >/dev/null 2>&1 || die "missing tool: $tool
  Arch:   pacman -S base-devel rust
  Debian: apt install build-essential rustc"
done
pkg-config --exists alsa 2>/dev/null || die "missing alsa dev headers (rodio/cpal need them)
  Arch:   pacman -S alsa-lib
  Debian: apt install libasound2-dev"
pkg-config --exists espeak-ng 2>/dev/null || die "missing espeak-ng dev files
  Arch:   pacman -S espeak-ng
  Debian: apt install libespeak-ng-dev"
pkg-config --exists wayland-client 2>/dev/null || die "missing wayland client dev files
  Arch:   pacman -S wayland
  Debian: apt install libwayland-dev"
log "dependencies OK"

# --- 2. parse requested key combo -------------------------------------------
normalize_mod() { # one mod token -> canonical form (spaces stripped)
    local m="${1^^}"
    m="${m// /}"
    case "$m" in
        MOD1|LALT|RALT|ALT)  echo "ALT" ;;
        MOD4|LSUPER|RSUPER|SUPER|LOGO|WIN|META) echo "SUPER" ;;
        MOD2|NUMLOCK)        echo "" ;;
        LCTRL|RCTRL|CTRL)    echo "CTRL" ;;
        LSHIFT|RSHIFT|SHIFT) echo "SHIFT" ;;
        *) echo "$m" ;;
    esac
}
normalize_key() { # key token -> canonical form (spaces stripped)
    local k="${1^^}"
    k="${k// /}"
    case "$k" in
        ESC) echo "ESCAPE" ;;
        *)   echo "$k" ;;
    esac
}
combo_of() { # "MODS, KEY" -> "ALT+SHIFT|ESCAPE" canonical string
    local mods key out="" tok
    IFS=',' read -r mods key _ <<< "$1"
    [[ -z "${key// /}" ]] && die "bad --key: '$1' (expected 'MODS, KEY')"
    IFS='|' read -ra toks <<< "${mods:-}"
    for tok in "${toks[@]:-}"; do
        tok="$(normalize_mod "$tok")"
        [[ -n "$tok" ]] && out="${out}+${tok}"
    done
    out="${out#+}"
    [[ -z "$out" ]] && out="NONE"
    echo "${out}|$(normalize_key "$key")"
}
REQ_COMBO="$(combo_of "$KEY")"
log "requested keybind: $KEY  →  [$REQ_COMBO]"

# --- 3. scan hyprland config (vars, sources, binds) ---------------------------
[[ -f "$CONF" ]] || die "hyprland config not found: $CONF"
declare -A VARS=()
declare -a SCAN_QUEUE=("$CONF")
declare -A VISITED=()

expand_vars() { # expand $name references using VARS (chained up to 5 passes)
    local s="$1" pass=0 name
    while (( pass++ < 5 )); do
        local changed=0
        for name in "${!VARS[@]}"; do
            while [[ "$s" == *"\$$name"* ]]; do
                s="${s//\$$name/${VARS[$name]}}"
                changed=1
            done
        done
        (( changed )) || break
    done
    echo "$s"
}

log "scanning hyprland config: $CONF"
# NB: regexes are held in single-quoted variables — inline \\$ escapes in
# [[ =~ ]] are parsed differently across bash versions and caused a subtle bug.
re_var='^[[:space:]]*\$([A-Za-z0-9_]+)[[:space:]]*=[[:space:]]*(.*)$'
re_source='^[[:space:]]*source[[:space:]]*=[[:space:]]*(.*)$'
re_bind='^[[:space:]]*bind[a-zA-Z]*[[:space:]]*=[[:space:]]*(.*)$'
while [[ ${#SCAN_QUEUE[@]} -gt 0 ]]; do
    f="${SCAN_QUEUE[0]}"; SCAN_QUEUE=("${SCAN_QUEUE[@]:1}")
    [[ -n "${VISITED[$f]:-}" ]] && continue
    VISITED[$f]=1
    [[ -f "$f" ]] || { warn "source not found, skipping: $f"; continue; }

    linenum=0
    while IFS= read -r line || [[ -n "$line" ]]; do
        linenum=$((linenum+1))
        # strip comments (whole-line or trailing after whitespace)
        line="${line%%#*}"
        [[ -z "${line// /}" ]] && continue

        # variable definitions:  $name = value
        if [[ "$line" =~ $re_var ]]; then
            VARS["${BASH_REMATCH[1]}"]="$(expand_vars "${BASH_REMATCH[2]}")"
            continue
        fi

        # source directives: follow them (relative to the including file's dir)
        if [[ "$line" =~ $re_source ]]; then
            src="$(expand_vars "${BASH_REMATCH[1]}")"
            src="${src/#\~/$HOME}"
            [[ "$src" != /* ]] && src="$(dirname "$f")/$src"
            SCAN_QUEUE+=("$src")
            continue
        fi
    done < "$f"
done

# second pass over all discovered files: find binds on the requested combo
declare -a FOUND=()   # "file:line:content"
for f in "${!VISITED[@]}"; do
    [[ -f "$f" ]] || continue
    linenum=0
    while IFS= read -r line || [[ -n "$line" ]]; do
        linenum=$((linenum+1))
        line="${line%%#*}"
        [[ "$line" =~ $re_bind ]] || continue
        rest="${BASH_REMATCH[1]}"
        rest="$(expand_vars "$rest")"
        IFS=',' read -r mods key cmd_rest <<< "$rest"
        [[ -z "${key// /}" ]] && continue
        combo="$(combo_of "${mods:-}, ${key}")"
        [[ "$combo" == "$REQ_COMBO" ]] && FOUND+=("$f:$linenum:$line")
    done < "$f"
done

ALREADY_BOUND=0
if [[ ${#FOUND[@]} -gt 0 ]]; then
    for entry in "${FOUND[@]}"; do
        fline="${entry%%:*}"; rest="${entry#*:}"; lineno="${rest%%:*}"; content="${rest#*:}"
        content="$(echo "$content" | sed 's/^[[:space:]]*//')"
        if [[ "$entry" == *"$BIN_NAME"* ]]; then
            log "bind already present ($fline:$lineno) — nothing to add"
            ALREADY_BOUND=1
        else
            die "KEYBIND CONFLICT: '$KEY' is already bound!

  $fline:$lineno
      $content

Choose another key with:  ./install.sh --key \"SUPER, comma\"
or free the binding above in your config first."
        fi
    done
fi

# --- 4. build ----------------------------------------------------------------
cd "$SCRIPT_DIR"
if [[ $SKIP_BUILD -eq 0 ]]; then
    log "building release binary"
    cargo build --release
fi
[[ -x "target/release/$BIN_NAME" ]] || die "binary not found: target/release/$BIN_NAME (build failed or --skip-build misuse)"

# --- 5. plan modifications -----------------------------------------------------
APPEND=()
if [[ $NO_RULES -eq 0 ]] && ! grep -rq 'class:\^('"$APP_ID"')\$' "$CONF" "$(dirname "$CONF")" 2>/dev/null; then
    # (the grep above is intentionally loose: rules for this app exist anywhere → skip)
    APPEND+=(
        "windowrule = float, class:^(${APP_ID})\$"
        "windowrule = size 440 170, class:^(${APP_ID})\$"
        "windowrule = pin, class:^(${APP_ID})\$"
    )
else
    [[ $NO_RULES -eq 0 ]] && log "window rules already present — skipping"
fi
if [[ $NO_BIND -eq 0 && $ALREADY_BOUND -eq 0 ]]; then
    if command -v "$BIN_NAME" >/dev/null 2>&1 || [[ ":$PATH:" == *":$BIN_DIR:"* ]]; then
        CMD="$BIN_NAME"
    else
        CMD="$BIN_DIR/$BIN_NAME"
    fi
    APPEND+=("bind = ${KEY}, exec, ${CMD}")
fi

# --- 6. apply -----------------------------------------------------------------
if [[ ${#APPEND[@]} -gt 0 ]]; then
    if [[ $DRY_RUN -eq 1 ]]; then
        log "dry-run: would append to $CONF:"
        printf '    %s\n' "${APPEND[@]}"
    else
        backup="$CONF.bak-$(date +%Y%m%d-%H%M%S)"
        cp "$CONF" "$backup"
        {
            printf '\n# --- hypr-speak (Speak Selection) --------------------------------\n'
            printf '%s\n' "${APPEND[@]}"
        } >> "$CONF"
        log "appended ${#APPEND[@]} line(s) to $CONF (backup: $backup)"
    fi
fi

if [[ $DRY_RUN -eq 0 ]]; then
    log "installing binary to $BIN_DIR"
    mkdir -p "$BIN_DIR"
    install -m 0755 "target/release/$BIN_NAME" "$BIN_DIR/$BIN_NAME"
    [[ ":$PATH:" == *":$BIN_DIR:"* ]] || warn "$BIN_DIR is not in your PATH"

    if [[ -n "${HYPRLAND_INSTANCE_SIGNATURE:-}" ]] && command -v hyprctl >/dev/null 2>&1; then
        hyprctl reload >/dev/null && log "hyprctl reloaded"
    fi
fi

log "done. Trigger with:  ${KEY}   (highlight text, press the key)"
if [[ $DRY_RUN -eq 1 ]]; then
    log "(dry run — nothing was changed)"
fi
exit 0
