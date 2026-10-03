# Architecture and release review — 2026-10-03

## Architecture

The module split is appropriate for a small application: CLI and selection
acquire input, textutil normalizes/chunks it, TtsEngine produces PCM, audio
owns playback, and egui renders the controls. Main currently owns engine
discovery and worker orchestration. Those responsibilities should move into
testable library modules before adding more engines or desktop backends.

The process has one UI thread and one synthesis worker. espeak uses global C
state; Piper launches a subprocess per sentence. Per-sentence Piper startup
can introduce audible gaps. The corrected worker waits for playback before
advancing the displayed sentence and remains alive during final playback to
accept speed restarts. This trades speculative prefetch for accurate sequencing.

## Findings and progress

| Severity | Finding | Status |
| --- | --- | --- |
| High | Multiple safe EspeakNg constructors could race on global FFI state | Fixed: atomic ownership gate; regression assertion |
| High | Lua installer array emitted malformed rule strings | Fixed: individually quoted lines |
| High | Recursive config variables could hang inside an unbounded loop | Fixed: bounded substitution passes |
| Medium | Dry run invoked cargo build | Fixed: no build and no binary prerequisite in dry run |
| Medium | Clipboard read accepted arbitrary MIME and unbounded bytes | Fixed: text MIME and bounded read |
| Medium | Piper sample rate narrowed unchecked, allowing zero/overflow | Fixed: checked conversion and validated range |
| Medium | Worker exited before final playback, losing speed restarts; display ran ahead | Fixed in worker; live desktop validation pending |
| Medium | Modifier order changed conflict detection | Fixed: sorted normalized modifier sets |
| Medium | Partial voice downloads could be treated as installed | Improved: temporary download destination and rename |
| Medium | Unsupported CPU silently selected x86_64 Piper | Fixed: explicit supported architecture check |
| Low | espeak byte count omitted NUL required by API | Fixed |
| Low | Missing license file, placeholder repository URL, incorrect Rust minimum, formatting drift | Fixed; CI and contribution guide added |
| Low | Demo and installer branding missing | Fixed: recorded neural synthesis and ASCII parrot |

## Outstanding release checks

Local verification: 36 Rust tests passed; strict Clippy, formatting, shell
syntax, ShellCheck error-level checks, and WAV metadata validation passed.
Release build and all 41 installer fixture checks passed, including Lua parsing.
The demo contains 10.93 seconds of mono 22050 Hz PCM16 audio.

- [ ] Exercise pause, speed restart, error, and close on a live desktop after worker changes.
- [ ] Run the new Ubuntu CI workflow on the public repository; add Fedora/Arch
  smoke builds before claiming those distributions tested.
- [ ] Harden config scanning further: source globs, canonical paths, variable
  token boundaries, Lua expressions/includes, exact command matching, and
  quoting paths containing shell/config metacharacters remain limitations.
- [ ] Verify downloaded Piper archives and voice weights with pinned checksums;
  current downloads rely on HTTPS and upstream availability.
- [ ] Add subprocess timeout/cancellation and worker state-machine tests.
- [ ] Review dependency advisories and redistribution terms before publishing binaries.
- [x] Public repository URL: https://github.com/rafaelzimmermann/parrot.

The code is prepared for public review as an early release, not certified for
all Linux desktops. Historical PLAN.md/TASKS.md claims describe earlier manual
work and do not substitute for current verification. This review supersedes
their completion claims and prefetch architecture description.
