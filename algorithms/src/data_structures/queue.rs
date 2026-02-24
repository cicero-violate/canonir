//! Queue — FIFO ring buffer of fixed capacity.
//!
//! Variables:
//!   buf  : Vec<Option<T>>  — circular backing array, length C
//!   head : usize           — index of next dequeue
//!   tail : usize           — index of next enqueue
//!   len  : usize           — current occupancy
//!
//! Equations:
//!   enqueue(x): buf[tail] = x,  tail = (tail+1) mod C,  len += 1
//!   dequeue():  x = buf[head],  head = (head+1) mod C,  len -= 1
//!   full  iff len == C
//!   empty iff len == 0

pub struct Queue<T> {
    buf: Vec<Option<T>>,
    head: usize,
    tail: usize,
    len: usize,
}

impl<T> Queue<T> {
    pub fn new(capacity: usize) -> Self {
        let mut buf = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            buf.push(None);
        }
        Self { buf, head: 0, tail: 0, len: 0 }
    }

    pub fn enqueue(&mut self, val: T) -> bool {
        if self.len == self.buf.len() {
            return false;
        }
        self.buf[self.tail] = Some(val);
        self.tail = (self.tail + 1) % self.buf.len();
        self.len += 1;
        true
    }

    pub fn dequeue(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        let val = self.buf[self.head].take();
        self.head = (self.head + 1) % self.buf.len();
        self.len -= 1;
        val
    }

    pub fn peek(&self) -> Option<&T> {
        self.buf[self.head].as_ref()
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn is_full(&self) -> bool {
        self.len == self.buf.len()
    }
    pub fn len(&self) -> usize {
        self.len
    }
}
