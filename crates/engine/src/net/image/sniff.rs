//! What an image is, decided from its bytes. The `Content-Type` a host sends is
//! only a claim, and the answer here is what the webview is told.

pub const SVG: &str = "image/svg+xml";

const SVG_SCAN: usize = 4096;

pub fn mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else if bytes.len() >= 12
        && &bytes[4..8] == b"ftyp"
        && matches!(&bytes[8..12], b"avif" | b"avis")
    {
        Some("image/avif")
    } else if is_svg(bytes) {
        Some(SVG)
    } else {
        None
    }
}

/// An SVG is text, so it is recognised by its root element after the prolog
/// (an XML declaration, comments, a doctype) rather than by a signature.
fn is_svg(bytes: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(SVG_SCAN)]);
    let mut rest = head.trim_start_matches('\u{feff}').trim_start();
    loop {
        let skipped = if let Some(after) = rest.strip_prefix("<?") {
            after.split_once("?>").map(|(_, tail)| tail)
        } else if let Some(after) = rest.strip_prefix("<!--") {
            after.split_once("-->").map(|(_, tail)| tail)
        } else if rest
            .get(..9)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("<!doctype"))
        {
            rest.split_once('>').map(|(_, tail)| tail)
        } else {
            break;
        };
        match skipped {
            Some(tail) => rest = tail.trim_start(),
            None => return false,
        }
    }
    rest.get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("<svg"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_are_told_apart_by_their_bytes_and_markup_is_not_one() {
        assert_eq!(mime(b"\x89PNG\r\n\x1a\n...."), Some("image/png"));
        assert_eq!(mime(&[0xff, 0xd8, 0xff, 0xe0, 0]), Some("image/jpeg"));
        assert_eq!(mime(b"GIF89a...."), Some("image/gif"));
        assert_eq!(mime(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(mime(b"\0\0\0\x1cftypavif\0\0\0\0"), Some("image/avif"));
        assert_eq!(
            mime(b"<?xml version=\"1.0\"?>\n<!-- logo -->\n<svg xmlns=\"http://www.w3.org/2000/svg\"/>"),
            Some(SVG)
        );
        assert_eq!(mime(b"<!DOCTYPE html><html><svg/></html>"), None);
        assert_eq!(mime(b"<html><body>not an image</body></html>"), None);
        assert_eq!(mime(b"{\"error\":true}"), None);
    }
}
