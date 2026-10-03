//! Text cleaning and sentence splitting for TTS.

/// Max characters we are willing to speak in one invocation.
pub const MAX_TEXT_LEN: usize = 20_000;
/// Max characters per sentence chunk (keeps speed changes responsive).
const MAX_CHUNK: usize = 280;

/// Strip control characters, collapse whitespace, cap length.
pub fn clean(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut last_ws = false;
    for ch in raw.chars() {
        let ch = match ch {
            // keep meaningful whitespace, drop other control chars
            '\n' | '\t' => ' ',
            c if c.is_control() => continue,
            c => c,
        };
        if ch.is_whitespace() {
            if !last_ws {
                out.push(' ');
            }
            last_ws = true;
        } else {
            out.push(ch);
            last_ws = false;
        }
    }
    let mut s = out.trim().to_string();
    if s.chars().count() > MAX_TEXT_LEN {
        let truncated: String = s.chars().take(MAX_TEXT_LEN).collect();
        s = format!("{truncated}\n\n[text truncated]");
    }
    s
}

/// Split text into speakable sentences (keeping terminators).
///
/// Rules:
/// - `. ! ? ; :` end a sentence when followed by whitespace, a closing
///   quote/bracket, or end-of-text (so `3.14` and `example.com` survive)
/// - newlines and CJK terminators (`。！？`) always end a sentence
/// - closing quotes/brackets right after a terminator are kept attached
///   (`hi."` stays together)
/// - a small abbreviation list (`e.g.`, `approx.`, `Mr.` …) suppresses splits
pub fn split_sentences(text: &str) -> Vec<String> {
    const ABBREVIATIONS: &[&str] = &[
        "e.g", "i.e", "etc", "approx", "al", "vs", "no", "nr", "mr", "mrs", "ms", "dr",
        "prof", "st", "inc", "ltd", "co", "fig", "resp", "min", "max", "sec",
    ];

    fn is_closing(c: char) -> bool {
        matches!(c, '"' | '\'' | '’' | '”' | '»' | ')' | ']' | '}' | '」' | '』')
    }

    fn ends_with_abbreviation(cur: &str) -> bool {
        let body = cur.strip_suffix('.').unwrap_or(cur);
        // last "word" allowing internal dots (e.g. → "e.g")
        let word: String = body
            .chars()
            .rev()
            .take_while(|c| c.is_alphabetic() || *c == '.')
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let word = word.trim_matches('.');
        !word.is_empty() && ABBREVIATIONS.contains(&word.to_lowercase().as_str())
    }

    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        cur.push(c);
        let terminator =
            matches!(c, '.' | '!' | '?' | ';' | ':' | '\n' | '。' | '！' | '？' | '…');
        if terminator {
            let always_ends = c == '\n' || matches!(c, '。' | '！' | '？');
            let next = chars.get(i + 1).copied();
            let ends = always_ends
                || match next {
                    None => true,
                    Some(n) => n.is_whitespace() || is_closing(n),
                };
            let abbrev = c == '.' && ends && ends_with_abbreviation(&cur);
            if ends && !abbrev {
                // absorb following closing quotes/brackets into this sentence
                let mut j = i + 1;
                while j < chars.len() && is_closing(chars[j]) {
                    cur.push(chars[j]);
                    j += 1;
                }
                i = j;
                push_sentence(&mut out, &mut cur);
                continue;
            }
        }
        i += 1;
    }
    push_sentence(&mut out, &mut cur);
    out
}

fn push_sentence(out: &mut Vec<String>, cur: &mut String) {
    let s = cur.trim().to_string();
    cur.clear();
    if s.is_empty() {
        return;
    }
    if s.chars().count() <= MAX_CHUNK {
        out.push(s);
        return;
    }
    // Hard-split overlong sentences at the last soft break.
    let mut start = 0usize;
    let chars: Vec<char> = s.chars().collect();
    while start < chars.len() {
        let end = (start + MAX_CHUNK).min(chars.len());
        let mut cut = end;
        if cut < chars.len() {
            for j in (start..end).rev() {
                if matches!(chars[j], ' ' | ',' | ';' | '-' | '—') {
                    cut = j + 1;
                    break;
                }
            }
        }
        let piece: String = chars[start..cut].iter().collect();
        let piece = piece.trim().to_string();
        if !piece.is_empty() {
            out.push(piece);
        }
        start = cut;
    }
}

/// Elide a string to at most `max` chars, appending an ellipsis when cut.
/// Safe for multi-byte text (operates on `char` boundaries).
pub fn elide(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max).collect();
        format!("{t}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_control_chars_and_whitespace() {
        assert_eq!(clean("  Hello \u{1}\t world \n\n again  "), "Hello world again");
    }

    #[test]
    fn caps_length() {
        let long = "a".repeat(MAX_TEXT_LEN + 500);
        let cleaned = clean(&long);
        assert!(cleaned.chars().count() < MAX_TEXT_LEN + 100);
        assert!(cleaned.contains("[text truncated]"));
    }

    #[test]
    fn splits_on_punctuation() {
        let s = split_sentences("One. Two? Three! Four; Five: Six");
        assert_eq!(s, vec!["One.", "Two?", "Three!", "Four;", "Five:", "Six"]);
    }

    #[test]
    fn keeps_abbreviations_and_numbers() {
        let s = split_sentences("Pi is 3.14 approx. e.g. like this. Done.");
        assert_eq!(s, vec!["Pi is 3.14 approx. e.g. like this.", "Done."]);
    }

    #[test]
    fn splits_on_newline() {
        let s = split_sentences("line one\nline two");
        assert_eq!(s, vec!["line one", "line two"]);
    }

    #[test]
    fn hard_splits_overlong_sentences() {
        let s = split_sentences(&format!("{}.", "word ".repeat(200)));
        assert!(s.len() >= 2);
        assert!(s.iter().all(|p| p.chars().count() <= 281));
    }

    #[test]
    fn empty_input() {
        assert!(split_sentences("   ").is_empty());
    }

    #[test]
    fn splits_after_closing_quote() {
        let s = split_sentences("He said \"hi.\" Then left.");
        assert_eq!(s, vec!["He said \"hi.\"", "Then left."]);
    }

    #[test]
    fn unicode_punctuation() {
        let s = split_sentences("こんにちは。世界！");
        assert_eq!(s, vec!["こんにちは。", "世界！"]);
    }

    #[test]
    fn newline_always_splits() {
        let s = split_sentences("no.dot but newline\nnext");
        assert_eq!(s, vec!["no.dot but newline", "next"]);
    }

    #[test]
    fn clean_only_control_chars_yields_empty() {
        assert_eq!(clean("\u{1}\u{2}\u{7f}"), "");
    }

    #[test]
    fn chunk_boundary_is_exact() {
        // exactly MAX_CHUNK chars + terminator → single sentence, no hard split
        let words: Vec<String> = (0..40).map(|i| format!("word{i:02}")).collect(); // 6 chars each + space
        let s = words.join(" ");
        let s = format!("{s}.");
        let out = split_sentences(&s);
        assert_eq!(out.len(), 1);
        assert!(out[0].chars().count() <= MAX_CHUNK + 1);
    }

    #[test]
    fn elide_short_and_long() {
        assert_eq!(elide("abc", 5), "abc");
        assert_eq!(elide("abcdef", 3), "abc…");
        // multibyte safe: 4 chars each 4 bytes
        assert_eq!(elide("日本語です", 2), "日本…");
    }
}
