use clap::Parser;

fn main() {
    let args = fixepub::Args::parse();
    fixepub::run(args);
}
