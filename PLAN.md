# parrot — "Speak Selection" for Hyprland (Wayland)

A lightweight, single-binary Rust utility following the macOS *Speak Selection* workflow:
hotkey → read primary selection → speak via TTS → small floating control overlay →
auto-exit when done.

Repo name: `parrot`. Binary name: `parrot`.

---

## 1. Goals & constraints

| Requirement | Approach |
|---|---|
| Pause/Play + velocity control (0.5×–2.0×) | egui overlay, rodio `Sink` |
| Read selection on startup | Wayland **primary selection** (fallback: clipboard) via `wl-clipboard-rs` |
| Auto-close on completion | watch `Sink::empty()` + worker `done` flag → `ViewportCommand::Close` |
| Easy install, no shortcut conflicts | `install.sh` parses `hyprland.conf` (resolves `$vars`, follows `source=`), aborts on conflict with exact file:line |
| Single binary / bundled deps | static-ish release build (`lto`, `strip`), TTS in-process, voice data embedded when feature `bundled` is on |

## 2. Architecture

```
                 Hyprland bind (ALT,Escape) ──exec──▶ parrot
                                                      │
 ┌────────────────────────────────────────────────────┼─────────────────────────────┐
 │ main.rs                                            ▼                             │
 │  CLI parse ─▶ selection.rs (primary ─▶ clipboard) ─▶ textutil.rs                 │
 │                                                     │ clean + sentence split      │
 │                                                     ▼                             │
 │                                              tts worker thread                    │
 │                        (espeak-ng FFI, wpm = 175 × speed, PCM i16 → WAV)         │
 │                                                     │ mpsc events + atomics       │
 │                                                     ▼                             │
 │                                              audio.rs (rodio Sink)               │
 │                                                     ▲ pause/play/clear            │
 │                                              ui.rs (eframe overlay)               │
 │                          Play/Pause ▸ slider ▸ progress ▸ auto-close ▸ Esc/✕      │
 └───────────────────────────────────────────────────────────────────────────────────┘
```

**Threads**
- *UI thread*: eframe event loop; owns `Arc<Shared>` (atomics: speed, restart request, done) + `mpsc::Receiver<Event>`.
- *TTS worker*: sole owner of the espeak-ng engine (not thread-safe); synthesizes sentence-by-sentence at the current speed; appends `SamplesBuffer` to the shared `Sink`; prefetches ahead (keeps ≤2 sentences queued).

**Speed control** — pitch-preserving (espeak words-per-minute), *not* tape-speed:
slider change → `restart_from = Some(current_idx)` → worker clears the sink and re-synthesizes
the current sentence at the new rate. Pause/Play is instant via `Sink::pause()/play()`.

**Auto-close** — when worker is done, sink is empty, and not paused: show "Done ✓" for 400 ms,
then `ctx.send_viewport_cmd(ViewportCommand::Close)`. Esc and ✕ close immediately.

## 3. TTS engine decision (the honest trade-off)

Requirement said: embed a Piper ONNX model via `include_bytes!`. Reality check:

| Option | Size | Quality | Complexity |
|---|---|---|---|
| **Piper ONNX** (`ort` runtime + espeak-ng phonemizer) | +50–100 MB binary | excellent | high — no maintained pure-Rust piper; still needs espeak-ng for phonemization |
| **espeak-ng** via direct FFI (hand-rolled bindings, ~8 symbols) | 0 MB extra | robotic but very intelligible | low |
| Pure-Rust formant synth | small | unusable | high |

**v1 ships espeak-ng FFI** behind a `TtsEngine` trait:

```rust
pub trait TtsEngine: Send {
    fn synthesize(&mut self, text: &str, speed: f32) -> Result<Pcm, TtsError>; // 16-bit mono
}
```

- Bindings are hand-rolled `extern "C"` decls (`espeak_Initialize/SetVoiceByName/
  SetParameter/SetSynthCallback/Synth/Terminate`) — no bindgen, no extra crate.
  `libespeak-ng.so` is linked normally (present on target machine).
- Feature `bundled` (ROADMAP/v1.1): `build.rs` copies `/usr/lib/espeak-ng-data` into the
  binary via `include_dir!`, extracted to `~/.cache/parrot/` at runtime and passed to
  `espeak_Initialize(path)` → zero external voice data. A future `piper` backend slots in
  behind the same trait (`include_bytes!` for the ONNX weights) without touching UI/audio.

## 4. Modules

| File | Responsibility |
|---|---|
| `src/main.rs` | CLI (`--text`, `--voice`, `--speed`, `--wav FILE`, `--verbose`), wiring, no-text → silent exit 0 |
| `src/lib.rs` | module tree + shared types |
| `src/selection.rs` | primary selection → clipboard fallback; sanitize; 20 000-char cap |
| `src/textutil.rs` | whitespace cleanup, sentence splitting (keep punctuation), unit-tested |
| `src/tts.rs` | `TtsEngine` trait + espeak-ng FFI backend + WAV container writer |
| `src/audio.rs` | rodio `OutputStream` + `Sink` wrapper (`play_wav`, pause/play, clear, empty) |
| `src/ui.rs` | eframe app: translucent borderless always-on-top overlay, controls, auto-close |
| `install.sh` | dependency check, bind-conflict detection (var-resolving, source-following), windowrules, backup + append, `hyprctl reload` |

## 5. Hyprland integration

App sets wayland `app_id = parrot` (eframe `NativeOptions::app_id`).
`install.sh` appends (idempotently, after backup):

```ini
windowrule = float, class:^(parrot)$
windowrule = size 440 170, class:^(parrot)$
windowrule = pin, class:^(parrot)$
bind = ALT, Escape, exec, parrot
```

Conflict detection normalizes a bind line to `(mods_set, key)` — resolving `$vars`,
mapping `MOD1→ALT`, `MOD4→SUPER` — and compares against the requested combo:
- same combo + different command → **abort**, print offending `file:line` + line text
- same combo + same command → "already bound", skip
- otherwise → append. Recurses into `source=` files; loop-safe.

## 6. Packaging

```toml
[profile.release]
lto = "fat"
codegen-units = 1
strip = true
panic = "abort"
```
rodio with `default-features = false` (no symphonia — we feed raw `SamplesBuffer`),
keeping the binary lean. Deployment: copy to `~/.local/bin`. Full static (musl +
static espeak-ng) documented as optional path for other machines.

## 7. Risks / mitigations

- egui/eframe API drift (0.36) → stick to stable core APIs; compiler-guided fixes.
- espeak-ng FFI from memory → ABI is plain ints/pointers; validate with `--wav` mode + unit test.
- No audio device in CI/headless → `--wav` mode is the test path; GUI shows an error state instead of crashing.
- Transparency needs compositor support → Hyprland supports ARGB surfaces (verified live).
