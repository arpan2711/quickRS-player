//! Error type for every format reader. Port of rsplay/errors.py: each variant is a one-line, user-facing message.

use std::fmt;

#[derive(Debug)]
pub enum Error {
    /// An encryption key is needed but not configured.
    KeyMissing(String),
    /// The PSARC archive is malformed or unsupported.
    Psarc(String),
    /// The SNG chart is malformed, unsupported or failed to decrypt.
    Sng(String),
    /// An arrangement could not be found or parsed.
    Arrangement(String),
    /// A feedpak is malformed or asks for something outside itself.
    Feedpak(String),
    /// A Wwise .wem file this converter doesn't understand.
    Wem(String),
    /// Anything else (bad keys file, bad JSON, ...).
    Other(String),
    Io(std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::KeyMissing(m)
            | Error::Psarc(m)
            | Error::Sng(m)
            | Error::Arrangement(m)
            | Error::Feedpak(m)
            | Error::Wem(m)
            | Error::Other(m) => f.write_str(m),
            Error::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl Error {
    pub fn is_key_missing(&self) -> bool {
        matches!(self, Error::KeyMissing(_))
    }
}
