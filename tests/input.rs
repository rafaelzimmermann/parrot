use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn positional_pipe_redirect_and_empty_input() {
    let dir = std::env::temp_dir().join(format!("parrot-input-{}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let input = dir.join("input.txt");
    std::fs::write(&input, "Hello from standard input.").unwrap();
    for mode in ["pipe", "file", "positional", "empty"] {
        let wav = dir.join(format!("{mode}.wav"));
        let mut command = Command::new(env!("CARGO_BIN_EXE_parrot"));
        command.args(["--engine", "espeak", "--wav"]).arg(&wav);
        // No Wayland connection is needed for any explicit input mode.
        command
            .env_remove("WAYLAND_DISPLAY")
            .env("XDG_RUNTIME_DIR", &dir);
        if mode == "file" {
            command.stdin(std::fs::File::open(&input).unwrap());
        } else {
            command.stdin(Stdio::piped());
        }
        if mode == "positional" {
            command.args(["--", "--literal text"]);
        }
        let mut child = command.spawn().unwrap();
        if mode == "pipe" {
            child
                .stdin
                .take()
                .unwrap()
                .write_all(b"Hello from a pipe.")
                .unwrap();
        } else if mode != "positional" {
            drop(child.stdin.take());
        }
        // Positional input must finish even with an open, unread stdin pipe.
        assert!(child.wait().unwrap().success());
        if mode == "empty" {
            assert!(!wav.exists());
        } else {
            let bytes = std::fs::read(&wav).unwrap();
            assert_eq!(&bytes[..4], b"RIFF");
            assert!(bytes.len() > 44);
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}
