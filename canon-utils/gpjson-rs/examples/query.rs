use gpjson_rs::{query_file, QueryOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() < 2 {
        eprintln!("Usage: query <ldjson-file> <jsonpath> [jsonpath...]");
        eprintln!("Example: query /path/to/file.ldjson '$.user.id'");
        std::process::exit(2);
    }

    let file = args.remove(0);
    let queries = args;

    let results = query_file(&file, &queries, QueryOptions::default())?;

    for (q_idx, result) in results.iter().enumerate() {
        println!("Query {q_idx}: {}", queries[q_idx]);
        for line_idx in 0..result.number_of_lines {
            let values = result.line_values(line_idx);
            let mut any = false;
            for value in values.iter() {
                if value.is_some() {
                    any = true;
                    break;
                }
            }
            if !any {
                continue;
            }
            println!("  line {line_idx}: {values:?}");
        }
    }

    Ok(())
}
