fn main() {
    // Default: scan the current directory.
    // Pass a path as first argument to scan a different directory.
    let target = std::env::args().nth(1).unwrap_or_else(|| ".".to_string());
    
    let root = Path::new(&target);
    
    eprintln!("Scanning: {}", root.display());
    
    let maps = repomap::build_repomap(root);
    
    if maps.is_empty() {
        eprintln!("No Rust files found.");
        return;
    }
    
    let rendered = repomap::render_repomap(&maps, root);
    let tokens = repomap::estimate_tokens(&rendered);
    
    println!("{}", rendered);
    eprintln!("---");
    eprintln!("{} files  |  {} symbols  |  ~{} tokens", maps.len(), maps.iter().map(|m| m.symbols.len()).sum::<usize>(), tokens);
}

pub mod extractor;

pub mod repomap;

pub mod symbol;