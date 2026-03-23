
mod config;
mod graph;
mod log;
mod runner;

// clap removed due to dead_code forbid conflict

// removed clap-based CLI to satisfy -F dead_code

fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Commands::Run { file } => {
            let tasks = config::load(&file).unwrap();
            let order = graph::topo_sort(&tasks).unwrap();
            let res = runner::run(&tasks, &order);
            log::write(&res).unwrap();
        }
        Commands::List { file } => {
            let tasks = config::load(&file).unwrap();
            for t in tasks {
                println!("{} -> {:?}", t.name, t.depends_on);
            }
        }
        Commands::Validate { file } => {
            match config::load(&file) {
                Ok(_) => println!("valid"),
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            }
        }
    }
}
