#[derive(Debug, Clone)]
pub struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<u8>,
    components: usize,
}

impl UnionFind {
    pub fn new(size: usize) -> Self {
        let parent = (0..size).collect::<Vec<_>>();
        let rank = vec![0u8; size];
        Self { parent, rank, components: size }
    }

    pub fn len(&self) -> usize {
        self.parent.len()
    }

    pub fn is_empty(&self) -> bool {
        self.parent.is_empty()
    }

    pub fn components(&self) -> usize {
        self.components
    }

    pub fn find(&mut self, x: usize) -> usize {
        let mut root = x;
        while self.parent[root] != root {
            root = self.parent[root];
        }

        let mut cur = x;
        while self.parent[cur] != cur {
            let next = self.parent[cur];
            self.parent[cur] = root;
            cur = next;
        }

        root
    }

    pub fn union(&mut self, a: usize, b: usize) -> bool {
        let mut ra = self.find(a);
        let mut rb = self.find(b);
        if ra == rb {
            return false;
        }

        if self.rank[ra] < self.rank[rb] {
            std::mem::swap(&mut ra, &mut rb);
        }
        self.parent[rb] = ra;
        if self.rank[ra] == self.rank[rb] {
            self.rank[ra] = self.rank[ra].saturating_add(1);
        }
        self.components = self.components.saturating_sub(1);
        true
    }

    pub fn labels(&mut self) -> Vec<usize> {
        (0..self.parent.len()).map(|i| self.find(i)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::UnionFind;

    #[test]
    fn union_find_merges_components() {
        let mut uf = UnionFind::new(6);
        assert_eq!(uf.components(), 6);

        assert!(uf.union(0, 1));
        assert!(uf.union(1, 2));
        assert!(uf.union(3, 4));
        assert_eq!(uf.components(), 3);

        let r0 = uf.find(0);
        let r2 = uf.find(2);
        let r3 = uf.find(3);
        let r4 = uf.find(4);
        assert_eq!(r0, r2);
        assert_eq!(r3, r4);
        assert_ne!(r0, r3);
    }

    #[test]
    fn redundant_union_is_noop() {
        let mut uf = UnionFind::new(3);
        assert!(uf.union(0, 1));
        assert!(!uf.union(0, 1));
        assert_eq!(uf.components(), 2);
    }
}
