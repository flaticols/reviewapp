/// Percent-encodes everything but RFC 3986 unreserved characters and, when
/// `keep_slashes`, `/`.
pub(crate) fn encode(text: &str, keep_slashes: bool) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        let unreserved = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~');
        if unreserved || (keep_slashes && byte == b'/') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// Decodes `%XX` escapes and `+` as a space; malformed escapes stay as written.
pub(crate) fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut ix = 0;
    while ix < bytes.len() {
        match bytes[ix] {
            b'%' if ix + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[ix + 1..ix + 3]).ok();
                match hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                    Some(byte) => {
                        decoded.push(byte);
                        ix += 3;
                        continue;
                    }
                    None => decoded.push(b'%'),
                }
            }
            b'+' => decoded.push(b' '),
            byte => decoded.push(byte),
        }
        ix += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_paths_with_spaces_and_unicode() {
        let path = "/Users/me/Code Review/src/naïve#1.rs";
        assert_eq!(decode(&encode(path, false)), path);
        assert_eq!(encode("a b/c", true), "a%20b/c");
        assert_eq!(decode("100%"), "100%");
        assert_eq!(decode("%zz"), "%zz");
    }
}
