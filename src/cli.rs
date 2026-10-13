use clap::Parser;
use language_tags::LanguageTag;

#[derive(Parser, Debug)]
#[command(version, about = "Repair EPUB files for Kindle", long_about = None)]
pub struct Args {
    #[arg(required = true)]
    pub filenames: Vec<std::path::PathBuf>,
    /// Language to use when package language metadata is missing or invalid.
    #[arg(long, default_value = "en", value_parser = parse_language)]
    pub language: String,
    /// Replace existing output files after a successful repair.
    #[arg(long)]
    pub force: bool,
}

fn parse_language(value: &str) -> Result<String, String> {
    let tag = LanguageTag::parse(value).map_err(|error| error.to_string())?;
    tag.validate().map_err(|error| error.to_string())?;
    Ok(value.to_owned())
}
