//! CPU Merkle tree over fixed-size blocks using SHA-256.
//!
//! Variables:
//!   L       = leaf count (must be power of 2)
//!   H       = 32 bytes  (SHA-256 digest size)
//!   tree[]  = flat array of 2*L nodes, each H bytes
//!             tree[L+i] = SHA256(block_i)        (leaves)
//!             tree[k/2] = SHA256(tree[k] || tree[k+1]) (parents)
//!
//! Equations:
//!   tree[L + i] = SHA256(data[i])
//!   tree[j]     = SHA256(tree[2j] || tree[2j+1])   for j in (L-1)..=1
//!   root        = tree[1]   (tree[0] unused, 1-indexed convention)
//!
//! Complexity: O(L) SHA-256 calls, O(log L) levels

use sha2::{Digest, Sha256};

pub const HASH_SIZE: usize = 32;

pub struct MerkleTree {
    pub nodes: Vec<[u8; HASH_SIZE]>, // length 2*L, 1-indexed (node[0] unused)
    pub leaf_count: usize,
}

impl MerkleTree {
    /// Build a Merkle tree from a slice of equal-sized blocks.
    /// leaf_count must be a power of 2.
    pub fn build(blocks: &[&[u8]]) -> Self {
        let l = blocks.len();
        assert!(l.is_power_of_two(), "leaf count must be power of 2");
        let mut nodes = vec![[0u8; HASH_SIZE]; 2 * l];

        // hash leaves
        for (i, block) in blocks.iter().enumerate() {
            let digest = Sha256::digest(block);
            nodes[l + i].copy_from_slice(&digest);
        }

        // build parents bottom-up
        for j in (1..l).rev() {
            let mut h = Sha256::new();
            h.update(nodes[2 * j]);
            h.update(nodes[2 * j + 1]);
            nodes[j].copy_from_slice(&h.finalize());
        }

        Self { nodes, leaf_count: l }
    }

    /// Root hash (tree[1]).
    pub fn root(&self) -> &[u8; HASH_SIZE] {
        &self.nodes[1]
    }

    /// Proof path for leaf i: sibling hashes from leaf to root.
    pub fn proof(&self, mut i: usize) -> Vec<[u8; HASH_SIZE]> {
        let mut path = Vec::new();
        i += self.leaf_count;
        while i > 1 {
            let sibling = if i % 2 == 0 { i + 1 } else { i - 1 };
            path.push(self.nodes[sibling]);
            i /= 2;
        }
        path
    }

    /// Verify a leaf against a proof path and expected root.
    pub fn verify(leaf_data: &[u8], mut index: usize, leaf_count: usize, proof: &[[u8; HASH_SIZE]], root: &[u8; HASH_SIZE]) -> bool {
        let mut current: [u8; HASH_SIZE] = Sha256::digest(leaf_data).into();
        index += leaf_count;
        for sibling in proof {
            let mut h = Sha256::new();
            if index % 2 == 0 {
                h.update(current);
                h.update(sibling);
            } else {
                h.update(sibling);
                h.update(current);
            }
            current = h.finalize().into();
            index /= 2;
        }
        &current == root
    }
}
