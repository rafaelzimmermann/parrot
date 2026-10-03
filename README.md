# parrot — `hypr-speak`

**Speak Selection for Hyprland.** Highlight any text, press `ALT+Escape`, and a small
floating overlay speaks your selection aloud — macOS-style — then closes itself.

```
┌──────────────────────────────────────────────┐
│ 🔊  Speaking                        [ ✕ ]    │
│                                              │
│ “The quick brown fox jumps over the laz…”    │
│ ▰▰▰▰▰▰▰▰▰▰▱▱▱▱▱▱▱▱▱▱▱▱▱▱▱▱▱▱▱▱▱▱▱▱▱  ▶ Pause │
│ 🐢 ───●──────────────── 🐇  1.00×           │
└──────────────────────────────────────────────┘
```

## Features

- **Wayland-native** — reads the *primary selection* (falls back to clipboard);
  exits silently in ~10 ms if nothing is selected
- **Fully offline** — espeak-ng synthesis, no network, no accounts
- **Pitch-preserving speed control** — 0.5×–2.0× slider re-synthesizes the current
  sentence on the fly
- **Sentence streaming** — long texts are split and spoken sentence-by-sentence
  (newlines, CJK `。！？`, quotes, and abbreviations handled)
- **Self-closing overlay** — borderless, translucent, always-on-top, pinned; auto-closes
  400 ms after playback ends (Esc / ✕ also work)
- **Safe installer** — resolves `$variables` and follows `source=` files in your
  `hyprland.conf`, aborts on keybind conflicts with `file:line`, makes a timestamped
  backup, and is idempotent (safe to re-run)
- **Lua config support** — detects Hyprland 0.56+ native Lua configs
  (`hyprland.lua`; where `hyprland.conf` is auto-generated) and edits the Lua
  source instead, with the same conflict detection (`hl.bind("var + Key", …)`
  patterns are resolved and normalized)

## Install

```sh
git clone … && cd parrot
./install.sh                 # deps check → conflict scan → build → config + ~/.local/bin
```

Choose a different key:

```sh
./install.sh --key "SUPER SHIFT, S"
```

Other flags: `--dry-run`, `--no-bind`, `--no-rules`, `--bin-dir DIR`, `--skip-build`, `--conf PATH`.

Requires: `rust` (1.85+), `gcc`, `pkg-config`, `alsa-lib`, `espeak-ng`, `wayland`
(see your distro's dev packages). Audio goes through PipeWire/Pulse/ALSA via `rodio`.

### Voice quality: espeak-ng (default) → Piper (neural)

espeak-ng is instant and tiny but robotic. For a natural voice, run:

    ./install.sh --voice en_US-lessac-medium

This downloads the official `piper` binary + a ~63 MB voice model into
`~/.local/share/hypr-speak/` (idempotent; ~5 MB binary, model of your choice).
hypr-speak then auto-prefers piper — espeak-ng remains the zero-download
fallback (`--engine espeak` forces it). Speed still works (piper `--length-scale`),
pitch preserved. Other voices: `de_DE-ramona-low`, `en_GB-alan-medium`, …
(see [piper-voices](https://huggingface.co/rhasspy/piper-voices)); preview the
exact URLs with `./install.sh --voice-url <name>`.

> **Note:** the installer binds the **absolute** binary path — Hyprland's own
> `PATH` (inherited from the session manager) often lacks `~/.local/bin`.
> Alternatively add it: `systemctl --user import-environment PATH` after
> exporting it in your profile, then re-run the installer.

## Usage

| Action | How |
|---|---|
| Speak selection | select text, press the keybind |
| Pause / resume | button (or click overlay) |
| Speed | drag the slider; current sentence restarts at the new rate |
| Dismiss | `Esc`, `✕`, or just wait for auto-close |

CLI extras:

```sh
hypr-speak --text "hello"        # skip clipboard
hypr-speak --voice de --speed 1.5
hypr-speak --text "test" --wav out.wav   # headless render, no GUI
hypr-speak --verbose             # crate logs only
```

## Uninstall

Remove the `# --- hypr-speak` block from `hyprland.conf` (a `.bak-*` backup sits
next to it), `hyprctl reload`, and delete `~/.local/bin/hypr-speak`.

## Development

```sh
cargo test              # 30 unit tests (text splitting, FFI, WAV, CLI, audio)
bash tests/install.sh.test   # 22 installer fixture tests (tmp dirs, never touches your config)
cargo build --release
```

Layout: `src/textutil.rs` (cleanup/split) · `src/selection.rs` (wl-clipboard) ·
`src/tts.rs` (espeak-ng FFI behind `TtsEngine` trait) · `src/audio.rs` (rodio player) ·
`src/ui.rs` (egui overlay) · `src/cli.rs` (flags) · `src/main.rs` (worker thread wiring).

### Design notes

- **espeak-ng via FFI** instead of embedded Piper ONNX: keeps the binary lean and
  dependency-free (`libespeak-ng.so` is ~1 MB and everywhere); the `TtsEngine` trait
  leaves room for a Piper/rhvoice drop-in later (see `PLAN.md`).
- **FFI pitfalls** worth remembering: `AUDIO_OUTPUT_SYNCHRONOUS = 2`, the synth
  callback must return **0** to continue on espeak-ng 1.52, and the global state
  forbids concurrent use (tests serialize behind a mutex).

## Roadmap

- [ ] Piper (neural) voice as an optional engine
- [ ] Config file (default voice/speed/keybind)
- [ ] Progress bar reflects actual audio position of the current sentence
