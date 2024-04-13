use clap::Parser;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(last = true)]
    filenames: Vec<String>,
}

fn main() {
    let args = Args::parse();

    for f in args.filenames {
        let path = Path::new(&f);
        let stem = path.file_stem().unwrap();
        let new_stem = format!("{}-fixed", stem.to_str().unwrap());
        let new_path = change_file_stem(path, &new_stem);
        let np = new_path.as_path();

        println!("Fixing {} as {}", f, np.to_string_lossy());
        fix(&f, np)
    }

    println!("Done");

    
}

fn change_file_stem(original_path: &Path, new_stem: &str) -> PathBuf {
    let mut new_path = PathBuf::new();

    if let Some(parent) = original_path.parent() {
        new_path.push(parent);
    }

    // Create the new filename by combining the new stem with the original extension
    let new_filename = match original_path.extension() {
        Some(extension) => format!("{}.{}", new_stem, extension.to_string_lossy()),
        None => new_stem.to_string(),
    };

    new_path.push(new_filename);
    new_path
}

fn fix(filename: &str, output_filename: &Path) {

}