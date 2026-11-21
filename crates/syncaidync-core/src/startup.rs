//! Detect agent startup prompts (trust folder, sign in) in terminal output.

/// A phrase to look for in normalized terminal text, and the message to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Marker {
    pub needle: &'static str,
    pub message: &'static str,
}

/// Strip CSI, OSC, and other common terminal escape sequences.
pub fn strip_ansi(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b {
            if i + 1 >= bytes.len() {
                // Lone ESC at the end: drop it.
                break;
            }
            match bytes[i + 1] {
                b'[' => {
                    // CSI: ESC [ … final byte in @–~
                    i += 2;
                    while i < bytes.len() {
                        let b = bytes[i];
                        i += 1;
                        if (0x40..=0x7e).contains(&b) {
                            break;
                        }
                    }
                }
                b']' => {
                    // OSC: ESC ] … BEL or ESC \
                    i += 2;
                    while i < bytes.len() {
                        if bytes[i] == 0x07 {
                            i += 1;
                            break;
                        }
                        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                }
                _ => {
                    // Other two-byte escape: skip ESC and the next byte.
                    i += 2;
                }
            }
            continue;
        }
        // Copy the next UTF-8 character.
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Strip ANSI, collapse whitespace, and lower-case. Used on the last 4 KiB of output.
pub fn normalize_tail(text: &str) -> String {
    let stripped = strip_ansi(text);
    let mut out = String::with_capacity(stripped.len());
    let mut prev_space = false;
    for ch in stripped.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            for c in ch.to_lowercase() {
                out.push(c);
            }
            prev_space = false;
        }
    }
    out.trim().to_string()
}

/// Find the first marker whose needle appears in the normalized text.
pub fn find_marker<'a>(normalized: &str, markers: &'a [Marker]) -> Option<&'a Marker> {
    markers.iter().find(|m| normalized.contains(m.needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_colours_and_cursor() {
        let s = "\x1b[31mred\x1b[0m \x1b[2J\x1b[Hhi";
        assert_eq!(strip_ansi(s), "red hi");
    }

    #[test]
    fn strip_osc_title() {
        let s = "\x1b]0;title\x07body\x1b]0;other\x1b\\done";
        assert_eq!(strip_ansi(s), "bodydone");
    }

    #[test]
    fn lone_esc_at_end() {
        assert_eq!(strip_ansi("ok\x1b"), "ok");
    }

    #[test]
    fn normalize_and_find() {
        let raw = "\x1b[1mDo You Trust the Files in this Folder?\x1b[0m";
        let n = normalize_tail(raw);
        assert!(n.contains("do you trust the files in this folder"));
        let markers = [Marker {
            needle: "do you trust the files in this folder",
            message: "Asking whether to trust this folder",
        }];
        let m = find_marker(&n, &markers).unwrap();
        assert_eq!(m.message, "Asking whether to trust this folder");
    }
}
