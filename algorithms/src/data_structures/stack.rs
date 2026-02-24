//! Stack — LIFO structure backed by Vec.
//!
//! Variables:
//!   data : Vec<T>  — backing storage
//!   N    : usize   — current number of elements = data.len()
//!
//! Equations:
//!   push(x): data[N] = x,  N' = N + 1       O(1) amortised
//!   pop():   N' = N - 1,   returns data[N-1] O(1)
//!   peek():  returns &data[N-1]              O(1)

pub struct Stack<T> {
    data: Vec<T>,
}

impl<T> Stack<T> {
    pub fn new() -> Self {
        Self { data: Vec::new() }
    }
    pub fn push(&mut self, val: T) {
        self.data.push(val);
    }
    pub fn pop(&mut self) -> Option<T> {
        self.data.pop()
    }
    pub fn peek(&self) -> Option<&T> {
        self.data.last()
    }
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
    pub fn len(&self) -> usize {
        self.data.len()
    }
}
