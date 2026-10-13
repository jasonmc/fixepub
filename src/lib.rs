pub mod cli;
pub mod encoding_matcher;
pub mod epub;
pub mod error;
mod xhtml_entities;

pub use cli::Args;
pub use error::FixError;

pub fn run(args: Args) -> Result<(), FixError> {
    for filename in args.filenames {
        let path = filename.as_path();
        let stem = path
            .file_stem()
            .ok_or_else(|| FixError::InvalidFileName(filename.to_string_lossy().into_owned()))?;
        let mut new_stem = stem.to_os_string();
        new_stem.push("-fixed");
        let new_path = epub::change_file_stem(path, &new_stem);
        let output_path = new_path.as_path();

        println!("{} ⟶ {}", filename.display(), output_path.display());
        epub::fix(&filename, output_path, &args.language, args.force)?;
    }
    Ok(())
}
