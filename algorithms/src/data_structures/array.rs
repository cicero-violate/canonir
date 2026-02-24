//! Fixed-size array with bounds-checked access.
//!
//! Variables:
//!   data : Vec<T>  — backing storage, length fixed at construction
//!   N    : usize   — length, immutable after new()
//!
//! Equations:
//!   get(i):  i < N  => Some(&data[i]),  else None    O(1)
//!   set(i,x):i < N  => data[i] = x,     else panic   O(1)
//!   rotate_left(k):  data[i] = data[(i+k) mod N]     O(N)

pub struct Array<T> {
    data: Vec<T>,
}

impl<T: Clone + Default> Array<T> {
    pub fn new(len: usize) -> Self {
        Self { data: vec![T::default(); len] }
    }

    pub fn from_vec(v: Vec<T>) -> Self {
        Self { data: v }
    }

    pub fn get(&self, i: usize) -> Option<&T> {
        self.data.get(i)
    }
    pub fn get_mut(&mut self, i: usize) -> Option<&mut T> {
        self.data.get_mut(i)
    }
    pub fn set(&mut self, i: usize, val: T) {
        self.data[i] = val;
    }
    pub fn len(&self) -> usize {
        self.data.len()
    }
    pub fn as_slice(&self) -> &[T] {
        &self.data
    }

    /// Rotate elements left by k positions: data[i] <- data[(i+k) % N]
    pub fn rotate_left(&mut self, k: usize) {
        let n = self.data.len();
        self.data.rotate_left(k % n);
    }

    /// Rotate elements right by k positions.
    pub fn rotate_right(&mut self, k: usize) {
        let n = self.data.len();
        self.data.rotate_right(k % n);
    }
}
