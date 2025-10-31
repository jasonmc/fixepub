pub mod cli;
pub mod encoding_matcher;
pub mod epub;
pub mod error;

pub use cli::Args;
pub use error::FixError;

use std::path::Path;

pub fn run(args: Args) -> Result<(), FixError> {
    for filename in args.filenames {
        let path = Path::new(&filename);
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| FixError::InvalidFileName(filename.clone()))?;
        let new_stem = format!("{stem}-fixed");
        let new_path = epub::change_file_stem(path, &new_stem);
        let output_path = new_path.as_path();

        println!("{} ⟶ {}", filename, output_path.to_string_lossy());
        epub::fix(&filename, output_path)?;
    }
    Ok(())
}
