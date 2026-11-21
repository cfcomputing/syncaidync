//! Random ids without a UUID crate.

/// UUID version 4 string from OS random bytes.
pub fn uuid_v4() -> String {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("os random");
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

/// Hex-encode `n` random bytes (so the string is `2*n` characters).
pub fn random_hex(n: usize) -> String {
    let mut b = vec![0u8; n];
    getrandom::fill(&mut b).expect("os random");
    let mut out = String::with_capacity(n * 2);
    for byte in b {
        out.push(hex_digit(byte >> 4));
        out.push(hex_digit(byte & 0x0f));
    }
    out
}

fn hex_digit(n: u8) -> char {
    char::from(if n < 10 { b'0' + n } else { b'a' + (n - 10) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_shape() {
        let id = uuid_v4();
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4");
        let variant = u8::from_str_radix(&id[19..20], 16).unwrap();
        assert!(variant == 8 || variant == 9 || variant == 10 || variant == 11);
    }

    #[test]
    fn random_hex_len() {
        assert_eq!(random_hex(32).len(), 64);
    }
}
