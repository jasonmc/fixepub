use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub enum FixError {
    Io(std::io::Error),
    Zip(zip::result::ZipError),
    ProgressTemplate(indicatif::style::TemplateError),
    InvalidFileName(String),
}

impl fmt::Display for FixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FixError::Io(err) => write!(f, "I/O error: {}", err),
            FixError::Zip(err) => write!(f, "ZIP error: {}", err),
            FixError::ProgressTemplate(err) => write!(f, "progress bar template error: {}", err),
            FixError::InvalidFileName(name) => write!(f, "invalid input filename: {name}"),
        }
    }
}

impl Error for FixError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            FixError::Io(err) => Some(err),
            FixError::Zip(err) => Some(err),
            FixError::ProgressTemplate(err) => Some(err),
            FixError::InvalidFileName(_) => None,
        }
    }
}

impl From<std::io::Error> for FixError {
    fn from(err: std::io::Error) -> Self {
        FixError::Io(err)
    }
}

impl From<zip::result::ZipError> for FixError {
    fn from(err: zip::result::ZipError) -> Self {
        FixError::Zip(err)
    }
}

impl From<indicatif::style::TemplateError> for FixError {
    fn from(err: indicatif::style::TemplateError) -> Self {
        FixError::ProgressTemplate(err)
    }
}
