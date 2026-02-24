//! Binary min-heap.
//!
//! Variables:
//!   data : Vec<T>  — heap array, 0-indexed
//!   N    : usize   — current size
//!
//! Equations:
//!   parent(i)     = (i - 1) / 2
//!   left_child(i) = 2*i + 1
//!   right_child(i)= 2*i + 2
//!
//!   Heap invariant: data[parent(i)] <= data[i]  for all i > 0
//!
//!   push(x):   data[N] = x,  sift_up(N),    N' = N+1   O(log N)
//!   pop_min(): swap(0, N-1), N' = N-1, sift_down(0)     O(log N)

pub struct MinHeap<T: Ord> {
    data: Vec<T>,
}

impl<T: Ord> MinHeap<T> {
    pub fn new() -> Self {
        Self { data: Vec::new() }
    }

    pub fn push(&mut self, val: T) {
        self.data.push(val);
        self.sift_up(self.data.len() - 1);
    }

    pub fn pop_min(&mut self) -> Option<T> {
        if self.data.is_empty() {
            return None;
        }
        let n = self.data.len() - 1;
        self.data.swap(0, n);
        let min = self.data.pop();
        if !self.data.is_empty() {
            self.sift_down(0);
        }
        min
    }

    pub fn peek_min(&self) -> Option<&T> {
        self.data.first()
    }
    pub fn len(&self) -> usize {
        self.data.len()
    }
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    fn sift_up(&mut self, mut i: usize) {
        while i > 0 {
            let p = (i - 1) / 2;
            if self.data[p] <= self.data[i] {
                break;
            }
            self.data.swap(p, i);
            i = p;
        }
    }

    fn sift_down(&mut self, mut i: usize) {
        let n = self.data.len();
        loop {
            let mut smallest = i;
            let l = 2 * i + 1;
            let r = 2 * i + 2;
            if l < n && self.data[l] < self.data[smallest] {
                smallest = l;
            }
            if r < n && self.data[r] < self.data[smallest] {
                smallest = r;
            }
            if smallest == i {
                break;
            }
            self.data.swap(i, smallest);
            i = smallest;
        }
    }
}
