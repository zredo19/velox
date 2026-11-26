use std::collections::VecDeque;

/// Fixed-capacity byte ring buffer for bounded scrollback.
///
/// Never exceeds `max_bytes` bytes. If a single `chunk` exceeds capacity,
/// only the last `max_bytes` bytes are kept.
/// If `max_bytes == 0`, all data is silently discarded.
pub struct RingScrollback {
    buf: VecDeque<u8>,
    max_bytes: usize,
    /// Total bytes ever pushed (monotonic stream offset), used to de-duplicate live output
    /// against a scrollback snapshot.
    total: u64,
}

impl RingScrollback {
    /// Creates a new `RingScrollback` with the given maximum byte capacity.
    pub fn new(max_bytes: usize) -> Self {
        Self {
            buf: VecDeque::with_capacity(max_bytes.min(1024 * 1024)), // Cap initial capacity at 1MB
            max_bytes,
            total: 0,
        }
    }

    /// Appends a chunk of bytes to the buffer, evicting oldest bytes if necessary.
    pub fn push(&mut self, chunk: &[u8]) {
        self.total = self.total.wrapping_add(chunk.len() as u64);
        if self.max_bytes == 0 {
            return;
        }

        let chunk_len = chunk.len();
        if chunk_len >= self.max_bytes {
            // Chunk is larger than capacity: keep only the last max_bytes bytes
            let start = chunk_len - self.max_bytes;
            self.buf.clear();
            self.buf.extend(&chunk[start..]);
            return;
        }

        // Check if we need to evict bytes to make room
        let new_len = self.buf.len() + chunk_len;
        if new_len > self.max_bytes {
            let excess = new_len - self.max_bytes;
            self.buf.drain(0..excess);
        }

        self.buf.extend(chunk);
    }

    /// Returns a snapshot of all bytes in chronological order.
    pub fn snapshot(&self) -> Vec<u8> {
        self.buf.iter().copied().collect()
    }

    /// Stream offset just past the last byte ever pushed.
    pub fn total(&self) -> u64 {
        self.total
    }

    /// Clears all bytes from the buffer.
    pub fn clear(&mut self) {
        self.buf.clear();
    }

    /// Returns the current number of bytes in the buffer.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Returns true if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_scrollback_preserves_order_within_capacity() {
        let mut rb = RingScrollback::new(100);
        rb.push(b"hello");
        rb.push(b" world");
        assert_eq!(rb.snapshot(), b"hello world");
    }

    #[test]
    fn ring_scrollback_evicts_oldest_bytes_when_overflowing() {
        let mut rb = RingScrollback::new(10);
        rb.push(b"1234567890"); // 10 bytes, fills exactly
        rb.push(b"AB"); // 2 bytes, should evict "12"
        assert_eq!(rb.snapshot(), b"34567890AB");
    }

    #[test]
    fn ring_scrollback_handles_chunk_larger_than_capacity() {
        let mut rb = RingScrollback::new(5);
        rb.push(b"1234567890"); // 10 bytes, capacity 5
        assert_eq!(rb.snapshot(), b"67890"); // Only last 5 bytes kept
    }

    #[test]
    fn ring_scrollback_zero_capacity_discards_all() {
        let mut rb = RingScrollback::new(0);
        rb.push(b"hello");
        rb.push(b"world");
        assert!(rb.is_empty());
        assert_eq!(rb.len(), 0);
        assert_eq!(rb.snapshot(), Vec::<u8>::new());
    }

    #[test]
    fn ring_scrollback_clear_resets_buffer() {
        let mut rb = RingScrollback::new(100);
        rb.push(b"hello");
        rb.clear();
        assert!(rb.is_empty());
        assert_eq!(rb.len(), 0);
        rb.push(b"world");
        assert_eq!(rb.snapshot(), b"world");
    }

    #[test]
    fn ring_scrollback_len_is_accurate() {
        let mut rb = RingScrollback::new(100);
        assert_eq!(rb.len(), 0);
        rb.push(b"hello");
        assert_eq!(rb.len(), 5);
        rb.push(b" world");
        assert_eq!(rb.len(), 11);
    }

    #[test]
    fn ring_scrollback_exact_capacity_boundary() {
        let mut rb = RingScrollback::new(10);
        rb.push(b"12345"); // 5 bytes
        rb.push(b"67890"); // 5 bytes, exactly at capacity
        assert_eq!(rb.len(), 10);
        assert_eq!(rb.snapshot(), b"1234567890");
    }
}
