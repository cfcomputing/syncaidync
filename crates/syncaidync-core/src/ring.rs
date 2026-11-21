//! Fixed-capacity byte ring used as a terminal replay buffer.

/// A byte ring of fixed capacity. Oldest bytes are dropped on overflow.
#[derive(Debug, Clone)]
pub struct ByteRing {
    buf: Vec<u8>,
    capacity: usize,
    /// Absolute number of bytes ever pushed (for start offset).
    total: u64,
}

impl ByteRing {
    pub fn new(capacity: usize) -> Self {
        Self {
            buf: Vec::with_capacity(capacity.min(4096)),
            capacity,
            total: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.buf.len().min(self.capacity)
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn push(&mut self, data: &[u8]) {
        if self.capacity == 0 || data.is_empty() {
            return;
        }
        if data.len() >= self.capacity {
            // Keep only the tail that fits.
            self.buf.clear();
            self.buf
                .extend_from_slice(&data[data.len() - self.capacity..]);
            self.total = self.total.saturating_add(data.len() as u64);
            return;
        }
        self.buf.extend_from_slice(data);
        if self.buf.len() > self.capacity {
            let overflow = self.buf.len() - self.capacity;
            self.buf.drain(0..overflow);
        }
        self.total = self.total.saturating_add(data.len() as u64);
    }

    /// Bytes currently held, starting on a UTF-8 character boundary.
    pub fn snapshot(&self) -> Vec<u8> {
        if self.buf.is_empty() {
            return Vec::new();
        }
        let mut start = 0;
        while start < self.buf.len() && is_utf8_continuation(self.buf[start]) {
            start += 1;
        }
        self.buf[start..].to_vec()
    }
}

fn is_utf8_continuation(b: u8) -> bool {
    (b & 0xC0) == 0x80
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_around() {
        let mut r = ByteRing::new(8);
        r.push(b"abcdefgh");
        assert_eq!(r.snapshot(), b"abcdefgh");
        r.push(b"XY");
        assert_eq!(r.len(), 8);
        assert_eq!(r.snapshot(), b"cdefghXY");
    }

    #[test]
    fn exact_capacity() {
        let mut r = ByteRing::new(4);
        r.push(b"1234");
        assert_eq!(r.len(), 4);
        assert_eq!(r.snapshot(), b"1234");
    }

    #[test]
    fn utf8_split_at_cut() {
        let mut r = ByteRing::new(5);
        // "é" is two bytes (C3 A9). Push "ab" + "é" + "cd" = 2+2+2 = 6 → keep last 5.
        // Bytes: 61 62 C3 A9 63 64 → after overflow drop 61: 62 C3 A9 63 64
        // If we had dropped mid-character: start with A9 (continuation) → skip to 63 64?
        // Actually: push all at once smaller pieces to force split.
        r.push(b"ab");
        r.push(&[0xC3, 0xA9]); // é
        r.push(b"cd");
        // capacity 5: total 6 bytes, drop first → starts at C3?
        // ab(2)+é(2)+cd(2)=6, drain 1 → starts with 62='b', then C3 A9 63 64 — fine
        // Force cut on continuation: capacity 3 after writing é alone then more
        let mut r2 = ByteRing::new(3);
        r2.push(&[0xC3, 0xA9, b'x', b'y']); // 4 bytes → keep last 3: A9 78 79
        let snap = r2.snapshot();
        // A9 is continuation → skip → "xy"
        assert_eq!(snap, b"xy");
        assert!(std::str::from_utf8(&snap).is_ok());
    }
}
