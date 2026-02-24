//! Bump-pointer arena allocator.
//!
//! Variables:
//!   buf : Vec<u8>  — backing slab, allocated once at construction
//!   len : usize    — current bump offset (next free byte)
//!   cap : usize    — total capacity = buf.len()
//!
//! Equations:
//!   alloc(n, align):
//!     pad  = (align - len % align) % align
//!     start= len + pad
//!     len' = start + n  ,  len' <= cap  (else panic)
//!     returns &mut buf[start .. start+n]
//!
//!   reset():
//!     len = 0          O(1) — no individual frees needed
//!
//!   utilisation = len / cap

pub struct Arena {
    buf: Vec<u8>,
    len: usize,
}

impl Arena {
    /// Allocate a new arena with `capacity` bytes.
    pub fn new(capacity: usize) -> Self {
        Self { buf: vec![0u8; capacity], len: 0 }
    }

    /// Bump-allocate `count` values of type T (alignment respected).
    /// Returns a mutable slice into the arena's backing buffer.
    /// Panics if remaining capacity is insufficient.
    pub fn alloc_slice<T: Copy>(&mut self, count: usize) -> &mut [T] {
        let align = std::mem::align_of::<T>();
        let size = std::mem::size_of::<T>();

        let pad = (align - self.len % align) % align;
        let start = self.len + pad;
        let end = start + size * count;

        assert!(end <= self.buf.len(), "arena out of capacity");

        self.len = end;

        let ptr = unsafe { self.buf.as_mut_ptr().add(start) as *mut T };

        // SAFETY:
        // - `start` is aligned for T
        // - region [start, end) is within buf
        // - &mut self guarantees unique access
        unsafe { std::slice::from_raw_parts_mut(ptr, count) }
    }

    /// Allocate a single value of type T, initialised to `val`.
    pub fn alloc<T: Copy>(&mut self, val: T) -> &mut T {
        let slice = self.alloc_slice::<T>(1);
        slice[0] = val;
        &mut slice[0]
    }

    /// Reset bump offset to zero — O(1) free of all allocations.
    pub fn reset(&mut self) {
        self.len = 0;
    }

    /// Bytes used / bytes capacity.
    pub fn utilisation(&self) -> (usize, usize) {
        (self.len, self.buf.len())
    }
}
