//! Wayland selection reading: primary selection (highlighted text) with a
//! clipboard fallback, via `wl-clipboard-rs` (windowless wayland client).

use std::io::Read;
use wl_clipboard_rs::paste::{get_contents, ClipboardType, MimeType, Seat};

/// Read the text the user currently has highlighted.
///
/// Order: primary selection → regular clipboard. Returns `Ok(None)` when no
/// usable text exists anywhere (caller should exit silently).
pub fn get_selection_text() -> anyhow::Result<Option<String>> {
    match read_source(ClipboardType::Primary) {
        Ok(Some(t)) => Ok(Some(t)),
        _ => read_source(ClipboardType::Regular),
    }
}

fn read_source(source: ClipboardType) -> anyhow::Result<Option<String>> {
    match get_contents(source, Seat::Unspecified, MimeType::Text) {
        Ok((pipe, _content_type)) => {
            let mut bytes = Vec::new();
            pipe.take((crate::textutil::MAX_TEXT_LEN * 4) as u64)
                .read_to_end(&mut bytes)?;
            let buf = String::from_utf8_lossy(&bytes);
            let t = buf.trim().to_string();
            if t.is_empty() {
                Ok(None)
            } else {
                Ok(Some(t))
            }
        }
        Err(e) if is_benign_error(&e) => Ok(None),
        Err(e) => Err(anyhow::anyhow!("reading {source:?} selection: {e}")),
    }
}

/// Errors that simply mean "nothing to read" rather than real failures —
/// the caller treats them like an empty selection.
pub fn is_benign_error(e: &wl_clipboard_rs::paste::Error) -> bool {
    use wl_clipboard_rs::paste::Error;
    matches!(
        e,
        Error::ClipboardEmpty
            | Error::NoSeats
            | Error::NoMimeType
            | Error::PrimarySelectionUnsupported
            | Error::SeatNotFound
    )
}

#[cfg(test)]
mod tests {
    use super::is_benign_error;
    use wl_clipboard_rs::paste::Error;

    #[test]
    fn benign_variants_mean_empty() {
        assert!(is_benign_error(&Error::ClipboardEmpty));
        assert!(is_benign_error(&Error::NoSeats));
        assert!(is_benign_error(&Error::NoMimeType));
        assert!(is_benign_error(&Error::PrimarySelectionUnsupported));
        assert!(is_benign_error(&Error::SeatNotFound));
    }

    #[test]
    fn real_errors_are_not_benign() {
        let e = Error::MissingProtocol {
            name: "wl_data_control",
            version: 2,
        };
        assert!(!is_benign_error(&e));
    }
}
