use clap::Parser;

fn main() {
    let args = fixepub::Args::parse();
    if let Err(err) = fixepub::run(args) {
        eprintln!("error: {}", err);
        std::process::exit(1);
    }
}
