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
#   --voice NAME       Also install the piper neural voice (e.g. en_US-lessac-medium):
#                       downloads the piper binary + voice model (~63 MB) on first use
#   --voice-url NAME   Print the download URLs for NAME and exit
#   -h | --help        This help

set -euo pipefail

DEFAULT_KEY="ALT, Escape"
KEY="$DEFAULT_KEY"
CONF="${XDG_CONFIG_HOME:-$HOME/.config}/hypr/hyprland.conf"
BIN_DIR="$HOME/.local/bin"
VOICE=""
VOICE_DIR="$HOME/.local/share/hypr-speak/voices"
PIPER_DIR="$HOME/.local/share/hypr-speak/piper"
PIPER_VER="2023.11.14-2"
DRY_RUN=0 NO_BIND=0 NO_RULES=0 SKIP_BUILD=0
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_NAME="hypr-speak"
APP_ID="hypr-speak"

log()  { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mWARNING:\033[0m %s\n' "$*"; }
die()  { printf '\033[1;31mERROR:\033[0m %s\n' "$*" >&2; exit 1; }

# Map a piper voice name (lang-speaker-quality) to its download URL prefix.
voice_url_prefix() {
    local n="$1" lang rest speaker quality
    [[ "$n" =~ ^[a-z]{2,3}_[A-Z]{2}-[a-zA-Z0-9_]+-(x_low|low|medium|high)$ ]] || die "bad voice name: $n"
    lang="${n%%-*}"; rest="${n#*-}"
    quality="${rest##*-}"; speaker="${rest%-*}"
    case "$quality" in low|x_low|medium|high) ;; *) die "bad voice name: $n (expected lang-speaker-quality, e.g. en_US-lessac-medium)" ;; esac
    [[ -n "$lang" && -n "$speaker" && "$lang" != "$n" ]] || die "bad voice name: $n"
    echo "https://huggingface.co/rhasspy/piper-voices/resolve/main/${lang:0:2}/${lang}/${speaker}/${quality}/${n}"
}

dl() { # dl URL DEST
    command -v curl >/dev/null 2>&1 || die "curl is required for --voice"
    log "downloading $(basename "$2")"
    curl -fSL --retry 3 --connect-timeout 20 --max-time 600 --progress-bar -o "$2.part" "$1"
    mv "$2.part" "$2"
}

setup_voice() {
    local prefix n
    prefix="$(voice_url_prefix "$VOICE")"
    n="$VOICE"
    mkdir -p "$VOICE_DIR"
    if [[ -s "$VOICE_DIR/$n.onnx" && -s "$VOICE_DIR/$n.onnx.json" ]]; then
        log "voice $n: already installed"
    else
        dl "$prefix.onnx"      "$VOICE_DIR/$n.onnx"
        dl "$prefix.onnx.json" "$VOICE_DIR/$n.onnx.json"
        [[ -s "$VOICE_DIR/$n.onnx" ]] || die "voice model download failed (empty file)"
    fi
    if [[ -x "$BIN_DIR/piper" ]]; then
        log "piper binary: already installed"
    else
        local arch
        arch="$(uname -m)"
        case "$arch" in aarch64|x86_64) ;; *) die "unsupported Piper architecture: $arch" ;; esac
        mkdir -p "$PIPER_DIR" "$BIN_DIR"
        log "downloading piper ($PIPER_VER, $arch)"
        curl -fSL --retry 3 --progress-bar \
            "https://github.com/rhasspy/piper/releases/download/$PIPER_VER/piper_linux_${arch}.tar.gz" \
            | tar xz -C "$PIPER_DIR" --strip-components=1
        ln -sf "$PIPER_DIR/piper" "$BIN_DIR/piper"
        "$BIN_DIR/piper" --help >/dev/null 2>&1 || die "piper binary does not run on this system"
    fi
    log "neural voice ready: hypr-speak will use piper ($n) automatically"
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --dry-run)   DRY_RUN=1 ;;
        --key)       KEY="${2:?--key needs a value}"; shift ;;
        --conf)      CONF="${2:?--conf needs a value}"; shift ;;
        --bin-dir)   BIN_DIR="${2:?--bin-dir needs a value}"; shift ;;
        --no-bind)   NO_BIND=1 ;;
        --no-rules)  NO_RULES=1 ;;
        --skip-build) SKIP_BUILD=1 ;;
        --voice)     VOICE="${2:?--voice needs a value (e.g. en_US-lessac-medium)}"; shift ;;
        --voice-url) VOICE_URL_ONLY=1; VOICE="${2:?--voice-url needs a value}"; shift ;;
        -h|--help)   sed -n '2,/^set /{ /^#/s/^# \{0,1\}//p; }' "$0"; exit 0 ;;
        *)           die "unknown option: $1 (see --help)" ;;
    esac
    shift
done

if [[ "${VOICE_URL_ONLY:-0}" -eq 1 ]]; then
    u="$(voice_url_prefix "$VOICE")"
    echo "$u.onnx"
    echo "$u.onnx.json"
    exit 0
fi

printf '%s\n' '       __' '      / o)>' '     / /)' '    / / )    parrot' '   /_/|/     Speak your selection.' '      ||' ''

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
    read -ra toks <<< "${mods//|/ }"
    for tok in "${toks[@]:-}"; do
        tok="$(normalize_mod "$tok")"
        [[ -n "$tok" ]] && out="${out}+${tok}"
    done
    out="$(printf '%s' "${out#+}" | tr '+' '\n' | sort -u | paste -sd+ -)"
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

# Hyprland 0.56+ native Lua config: hyprland.conf is AUTOGENERATED from
# hyprland.lua; binds must be registered in the Lua file to take effect.
LUA_CONF=""
if [[ -f "${CONF%.conf}.lua" ]]; then
    LUA_CONF="${CONF%.conf}.lua"
    log "Lua config detected: $LUA_CONF (hyprland.conf is generated — editing the .lua)"
fi

expand_vars() { # expand $name references using VARS (chained up to 5 passes)
    local s="$1" pass=0 name
    while (( pass++ < 5 )); do
        local changed=0
        for name in "${!VARS[@]}"; do
            if [[ "$s" == *"\$$name"* ]]; then
                s="${s//\$$name/${VARS[$name]}}"
                changed=1
            fi
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

declare -a FOUND=()   # "file:line:content"

if [[ -n "$LUA_CONF" ]]; then
    # ---- Lua config scan -------------------------------------------------
    # resolves string vars (`local mainMod = "SUPER"`), then normalizes the
    # first argument of every hl.bind(...) to the same canonical form as REQ_COMBO.
    declare -A LVARS=()
    re_lvar='^[[:space:]]*local[[:space:]]+([A-Za-z0-9_]+)[[:space:]]*=[[:space:]]*"([^"]*)"[[:space:]]*(--.*)?$'
    re_lvar2='^[[:space:]]*([A-Za-z0-9_]+)[[:space:]]*=[[:space:]]*"([^"]*)"[[:space:]]*(--.*)?$'
    re_lbind='hl\.bind[[:space:]]*\('
    linenum=0
    while IFS= read -r line; do
        linenum=$((linenum+1))
        if [[ "$line" =~ $re_lvar || "$line" =~ $re_lvar2 ]]; then
            LVARS["${BASH_REMATCH[1]}"]="${BASH_REMATCH[2]}"
        fi
    done < "$LUA_CONF"

    linenum=0
    while IFS= read -r line; do
        linenum=$((linenum+1))
        [[ "$line" =~ $re_lbind ]] || continue
        # first arg: up to the first top-level comma
        arg="${line#*hl.bind(}"
        arg="${arg%%,*}"
        # expand `var .. " + X"` concatenations
        s="$arg"
        for v in "${!LVARS[@]}"; do s="${s//$v/${LVARS[$v]}}"; done
        s="${s//../ }"; s="${s//\"/ }"          # drop concat dots + quotes
        # tokens: first n-1 = mods, last = key; unresolved vars (braces etc.) → skip
        read -ra toks <<< "$s"
        [[ ${#toks[@]} -lt 1 ]] && continue
        key="${toks[${#toks[@]}-1]}"
        unset 'toks[${#toks[@]}-1]'
        mods=""
        for t in "${toks[@]}"; do
            [[ -z "${t// /}" || "$t" == "+" ]] && continue
            t="$(normalize_mod "$t")"; [[ -n "$t" ]] && mods="${mods}+${t}"
        done
        mods="$(printf '%s' "${mods#+}" | tr '+' '\n' | sort -u | paste -sd+ -)"; [[ -z "$mods" ]] && mods="NONE"
        combo="${mods}|$(normalize_key "$key")"
        [[ "$combo" == "$REQ_COMBO" ]] && FOUND+=("$LUA_CONF:$linenum:$line")
    done < "$LUA_CONF"
else
    # ---- classic hyprlang scan --------------------------------------------
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
fi

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

# --- 4. neural voice (optional --voice) -------------------------------------
if [[ -n "$VOICE" ]]; then
    if [[ $DRY_RUN -eq 1 ]]; then
        log "dry-run: would download piper + voice '$VOICE' into ~/.local/share/hypr-speak/"
    else
        setup_voice
    fi
fi

# --- 5. build ---------------------------------------------------------------
cd "$SCRIPT_DIR"
if [[ $SKIP_BUILD -eq 0 && $DRY_RUN -eq 0 ]]; then
    log "building release binary"
    cargo build --release --locked
fi
[[ $DRY_RUN -eq 1 || -x "target/release/$BIN_NAME" ]] || die "binary not found: target/release/$BIN_NAME (build failed or --skip-build misuse)"

# --- 5. plan modifications -----------------------------------------------------
APPEND=()
# NOTE: always bind the ABSOLUTE path — Hyprland's own PATH (inherited from the
# session/seat manager) usually does NOT include ~/.local/bin, even if the
# installer's shell does. A bare name silently fails to spawn.
CMD="$BIN_DIR/$BIN_NAME"
if [[ -n "$LUA_CONF" ]]; then
    TARGET="$LUA_CONF"
    if [[ $NO_RULES -eq 0 ]] && ! grep -Eq 'hypr-speak-overlay|class *= *["'\''\^]?hypr-speak' "$LUA_CONF"; then
        APPEND+=(
            '-- --- hypr-speak (Speak Selection) --------------------------------'
            'hl.window_rule({'
            '    name  = "hypr-speak-overlay",'
            '    match = { class = "^'"$APP_ID"'$" },'
            '    float = true,'
            '    size  = "440 170",'
            '    pin   = true,'
            '})'
        )
    else
        [[ $NO_RULES -eq 0 ]] && log "window rules already present — skipping"
    fi
    if [[ $NO_BIND -eq 0 && $ALREADY_BOUND -eq 0 ]]; then
        # translate "MODS, KEY" → Lua "MODS + KEY"
        LUA_KEY="$(echo "${KEY//,/ + }" | xargs)"
        APPEND+=("hl.bind(\"${LUA_KEY}\", hl.dsp.exec_cmd(\"${CMD}\"))")
    fi
else
    TARGET="$CONF"
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
        APPEND+=("bind = ${KEY}, exec, ${CMD}")
    fi
fi

# --- 6. apply -----------------------------------------------------------------
if [[ ${#APPEND[@]} -gt 0 ]]; then
    if [[ $DRY_RUN -eq 1 ]]; then
        log "dry-run: would append to $TARGET:"
        printf '    %s\n' "${APPEND[@]}"
    else
        backup="$TARGET.bak-$(date +%Y%m%d-%H%M%S)"
        cp "$TARGET" "$backup"
        {
            printf '\n'
            printf '%s\n' "${APPEND[@]}"
        } >> "$TARGET"
        log "appended ${#APPEND[@]} line(s) to $TARGET (backup: $backup)"
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
