use canon_event_store::read_binary_events;
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).unwrap();
    let path = Path::new(&path);

    let events = read_binary_events(path)?;

    for e in events {
        println!("{:#?}", e);
    }

    Ok(())
}
