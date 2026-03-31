use thiserror::Error;

#[derive(Debug, Error)]
pub enum FixError {
    #[error("I/O error: {0}")]
    Io(std::io::Error),
    #[error("ZIP error: {0}")]
    Zip(zip::result::ZipError),
    #[error("progress bar template error: {0}")]
    ProgressTemplate(indicatif::style::TemplateError),
    #[error("invalid input filename: {0}")]
    InvalidFileName(String),
}

impl From<std::io::Error> for FixError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<zip::result::ZipError> for FixError {
    fn from(err: zip::result::ZipError) -> Self {
        Self::Zip(err)
    }
}

impl From<indicatif::style::TemplateError> for FixError {
    fn from(err: indicatif::style::TemplateError) -> Self {
        Self::ProgressTemplate(err)
    }
}
