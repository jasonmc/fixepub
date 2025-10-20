pub mod cli;
pub mod encoding_matcher;
pub mod epub;

pub use cli::Args;

use std::path::Path;

pub fn run(args: Args) {
    for filename in args.filenames {
        let path = Path::new(&filename);
        let stem = path
            .file_stem()
            .expect("Failed to read file stem from input filename");
        let new_stem = format!("{}-fixed", stem.to_str().expect("Invalid UTF-8 in file stem"));
        let new_path = epub::change_file_stem(path, &new_stem);
        let output_path = new_path.as_path();

        println!("{} ⟶ {}", filename, output_path.to_string_lossy());
        epub::fix(&filename, output_path);
    }
}
