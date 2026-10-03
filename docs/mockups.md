# parrot — UI mockups & interaction flows

## 1. Main overlay (playing)

440×170 px, borderless, rounded 12 px, ~78 % opaque dark, always-on-top (pin), floats.

```
╭──────────────────────────────────────────────────────╮
│  ◉ parrot                                  ✕     │   ← drag anywhere; ✕ = close
│──────────────────────────────────────────────────────│
│  “The quick brown fox jumps over the lazy dog.”      │   ← current sentence, dim white,
│                                                      │      elided to 2 lines
│  ███████████████░░░░░░░░░░░░░░░░░░░░░░░░░  3 / 12    │   ← progress bar + sentence counter
│                                                      │
│  ┌──────────┐   Velocity                             │
│  │  ⏸ Pause │   ─────────●────────────    1.25×      │   ← Pause toggles to ▶ Play;
│  └──────────┘                                          │      slider 0.5×–2.0×, live
╰──────────────────────────────────────────────────────╯
```

Palette: bg `#101418` @ α 200, text `#E6E1CF`, accent (bar/button) `#56B6C2`,
dim text `#8A8578`, error `#E06C75`.

## 2. States

| State | Visual | Transition |
|---|---|---|
| **Playing** | button “⏸ Pause”, bar animates per sentence | slider/pause available |
| **Paused** | button “▶ Play”, bar frozen, text “Paused” appended to status | resume / close |
| **Done** | button disabled, “Done ✓”, bar 100 % | auto-close after 400 ms |
| **Error** (no audio device / TTS init fail) | red panel: message + “Close” | auto-close after 6 s |
| **No text** | *(no window at all)* | silent exit 0 |
| **Synth error mid-text** | status line shows warning, continues next sentence | — |

## 3. Lifecycle sequence

```
 user          Hyprland            parrot                espeak worker        rodio
  │ highlight text   │                  │                       │                 │
  │ ALT,Escape ─────▶│  exec parrot │                       │                 │
  │                  │─────────────────▶│ read primary sel      │                 │
  │                  │                  │ (empty? exit 0)       │                 │
  │                  │◀── map window ───│ spawn worker ────────▶│ synth s0 ──────▶│ play
  │                  │                  │◀──── SentenceStarted ─│ synth s1 (pref) │
  │                  │                  │   UI: progress/labels │                 │
  │  drag slider ─────────────────────▶│ restart_from=i ──────▶│ clear+resynth ─▶│
  │  click Pause ─────────────────────▶│ sink.pause() ───────────────────────────▶│
  │                  │                  │◀──── Done ────────────│                 │
  │                  │                  │  sink.empty()? ────────────────────────▶│ yes
  │                  │◀── unmapped ─────│ Close (after 400 ms)  │                 │
```

## 4. install.sh flow

```
        ┌────────────┐   missing    ┌──────────────────────────┐
        │ dep check  │─────────────▶│ print exact pkgs (Arch/  │
        │ cargo/alsa │               │ Debian names) + abort    │
        │ /espeak-ng │               └──────────────────────────┘
        └─────┬──────┘
              │ ok
        ┌─────▼──────────┐   conflict     ┌──────────────────────────────┐
        │ scan binds     │───────────────▶│ ABORT: file:line + line text │
        │ (resolve $vars,│                │ + suggested alternative key  │
        │  follow source)│                └──────────────────────────────┘
        └─────┬──────┘    already-bound ──▶ skip append, continue
              │ free
        ┌─────▼──────────┐
        │ backup conf    │  hyprland.conf.bak-YYYYmmdd-HHMMSS
        │ append rules   │  windowrule float/size/pin (idempotent)
        │ append bind    │  bind = ALT, Escape, exec, parrot
        │ build release  │  cargo build --release
        │ install bin    │  ~/.local/bin (PATH check)
        │ hyprctl reload │
        └────────────────┘
```

Flags: `--dry-run`, `--key "SUPER, comma"` (custom combo), `--no-bind`, `--no-rules`,
`--bin-dir DIR`, `--skip-build`.
