# TASKS — parrot (hypr-speak)

Legend: `[x]` done · `[~]` in progress · `[ ]` todo · `[-]` skipped (documented)

## T0 Recon ............................... DONE
- [x] toolchain, libs (alsa/espeak-ng/wayland), network — all OK
- [x] live Hyprland session confirmed; `ALT,Escape` free; config uses `$mainMod` vars

## T1 Planning
- [x] PLAN.md (architecture, decisions, risks)
- [x] docs/mockups.md (overlay states, sequence, install flow)

## T2 Scaffold
- [x] Cargo.toml (eframe 0.35, egui 0.35, rodio 0.22 no-default+playback, wl-clipboard-rs 0.9) + release profile
- [x] empty main compiles & links (`cargo check`)

## T3 Core modules
- [x] textutil.rs — cleanup + sentence split (newline/CJK/quotes/abbreviations) + 12 unit tests
- [x] selection.rs — primary → clipboard fallback, benign-error classification + tests
- [x] tts.rs — TtsEngine trait, espeak-ng FFI (hand-rolled, header-verified), WAV writer + 7 tests
      (found+fixed: SYNCHRONOUS=2 not 3; POS_CHARACTER=1; callback returns 0 to continue)
- [x] audio.rs — rodio 0.22 Player wrapper (Arc<Mutex<Player>> shared with worker) + 2 tests

## T4 UI
- [x] ui.rs — overlay per mockup: title/✕, sentence, progress, Pause/Play, velocity slider
- [x] auto-close on EOF (+400 ms), Esc/✕ close, error state (6 s auto-close)
- [x] transparent borderless always-on-top, app_id hypr-speak (with_app_id)

## T5 Wiring
- [x] main.rs — CLI via testable cli.rs (--text/--voice/--speed/--wav/--verbose/--help) + 6 tests
- [x] worker thread: sentence loop, speed restart, prefetch ≤2, events to UI

## T6 Build & test
- [x] cargo test: 30/30 unit tests green
- [x] cargo build --release clean (16.4 MB binary)
- [x] headless: --wav → valid 7 s WAV; aplay playback OK
- [ ] live: wl-copy primary → run app → window exists (hyprctl clients)
- [ ] live: auto-close observed (exit code + timing)
- [ ] live: screenshot via grimblast matches mockup layout
- [ ] live: no-selection → silent exit 0

## T7 install.sh
- [ ] dep check (cargo, gcc, pkg-config, alsa, espeak-ng)
- [ ] bind scan: resolve $vars, follow source=, MOD1/MOD4 mapping, conflict abort w/ file:line
- [ ] idempotent append (rules + bind), timestamped backup, hyprctl reload
- [ ] flags: --dry-run --key --no-bind --no-rules --bin-dir --skip-build
- [ ] fixture tests: conflict / already-bound / var-substitution / nested source (tmp HOME)
- [ ] --dry-run against real config

## T8 Docs & wrap-up
- [ ] README.md (usage, keybind, troubleshooting, roadmap)
- [ ] final TASKS status + summary

---
## Progress log
- T0/T1 complete — env verified live; plan+mockups written
- T2–T5 complete — all modules written; API drift vs egui 0.35/rodio 0.22 resolved
  (eframe App::ui, Player-not-Sink, ClipboardType, const Color32, corner_radius)
- T6: 30/30 unit tests; release build; --wav headless pipeline verified (valid WAV + audible)
  FFI bugs found via ctypes cross-check: output mode, position type, param order, callback
  return polarity — all fixed with tests. Next: live GUI test, install.sh, README.
