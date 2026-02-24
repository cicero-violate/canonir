//! Singly-linked list with owned nodes.
//!
//! Variables:
//!   head : Option<Box<Node<T>>>  — pointer to first node, None if empty
//!   N    : usize                 — number of nodes
//!
//! Equations:
//!   push_front(x): new_node.next = head,  head = new_node,  N' = N+1  O(1)
//!   pop_front():   head = head.next,  N' = N-1                         O(1)
//!   push_back(x):  walk to tail, tail.next = new_node                  O(N)

struct Node<T> {
    val: T,
    next: Option<Box<Node<T>>>,
}

pub struct LinkedList<T> {
    head: Option<Box<Node<T>>>,
    len: usize,
}

impl<T> LinkedList<T> {
    pub fn new() -> Self {
        Self { head: None, len: 0 }
    }

    pub fn push_front(&mut self, val: T) {
        let node = Box::new(Node { val, next: self.head.take() });
        self.head = Some(node);
        self.len += 1;
    }

    pub fn pop_front(&mut self) -> Option<T> {
        self.head.take().map(|node| {
            self.head = node.next;
            self.len -= 1;
            node.val
        })
    }

    pub fn push_back(&mut self, val: T) {
        let new_node = Box::new(Node { val, next: None });
        let mut cur = &mut self.head;
        while let Some(node) = cur {
            cur = &mut node.next;
        }
        *cur = Some(new_node);
        self.len += 1;
    }

    pub fn peek_front(&self) -> Option<&T> {
        self.head.as_ref().map(|n| &n.val)
    }

    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}
