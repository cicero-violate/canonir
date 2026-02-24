//! FFI bridge to cryptography/merkle_tree.cu via unified libgpu.a.
//!
//! Variables:
//!   leaf_count : usize      — number of pages, must be power of 2
//!   pages      : &[u8]      — flat buffer of leaf_count * PAGE_SIZE bytes
//!   tree       : Vec<u8>    — output flat array, 2*L nodes each 32 bytes
//!                             tree[1*32..(1+1)*32] = root (1-indexed)
//!
//! Equations:
//!   tree[(L+i)*32 .. (L+i+1)*32] = SHA256(pages[i*4096 .. (i+1)*4096])
//!   tree[j*32 .. (j+1)*32]       = SHA256(tree[2j] || tree[2j+1])
//!   root = tree[32..64]   (node index 1)

pub const PAGE_SIZE: usize = 4096;
pub const HASH_SIZE: usize = 32;

#[cfg(feature = "cuda")]
unsafe extern "C" {
    fn gpu_merkle_build(tree: *mut u8, leaf_count: u64, pages: *const u8);
}

/// Build a GPU Merkle tree over `pages` (each PAGE_SIZE bytes).
/// Returns flat node array (2*L nodes, 32 bytes each, 1-indexed).
/// Root is at bytes [32..64].
#[cfg(feature = "cuda")]
pub fn merkle_build_gpu(pages: &[u8]) -> Vec<u8> {
    assert_eq!(pages.len() % PAGE_SIZE, 0, "pages not aligned to PAGE_SIZE");
    let l = pages.len() / PAGE_SIZE;
    assert!(l.is_power_of_two(), "leaf count must be power of 2");

    // 2*L nodes, each HASH_SIZE bytes, 1-indexed so node 0 is unused
    let mut tree = vec![0u8; 2 * l * HASH_SIZE];
    unsafe {
        gpu_merkle_build(tree.as_mut_ptr(), l as u64, pages.as_ptr());
    }
    tree
}

/// Extract root hash from a built tree (node index 1).
pub fn root(tree: &[u8]) -> &[u8] {
    &tree[HASH_SIZE..HASH_SIZE * 2]
}
