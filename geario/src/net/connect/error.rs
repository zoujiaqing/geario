use std::io;

use crate::error::ErrorDiagnostic;

#[derive(thiserror::Error, Debug)]
pub enum ConnectError {
    /// Failed to resolve the hostname
    #[error("Failed resolving hostname: {0}")]
    Resolver(io::Error),

    /// No dns records
    #[error("No dns records found for the input")]
    NoRecords,

    /// Invalid input
    #[error("Invalid input")]
    InvalidInput,

    /// Unresolved host name
    #[error("Connector received `Connect` method with unresolved host")]
    Unresolved,

    /// Connection io error
    #[error("{0}")]
    Io(#[from] io::Error),
}

impl Clone for ConnectError {
    fn clone(&self) -> Self {
        match self {
            ConnectError::Resolver(err) => {
                ConnectError::Resolver(io::Error::new(err.kind(), format!("{err}")))
            }
            ConnectError::NoRecords => ConnectError::NoRecords,
            ConnectError::InvalidInput => ConnectError::InvalidInput,
            ConnectError::Unresolved => ConnectError::Unresolved,
            ConnectError::Io(err) => ConnectError::Io(io::Error::new(err.kind(), format!("{err}"))),
        }
    }
}

impl ErrorDiagnostic for ConnectError {
    fn signature(&self) -> &'static str {
        match self {
            ConnectError::InvalidInput => "geario-connect-InvalidInput",
            ConnectError::Resolver(_) => "geario-connect-Resolver",
            ConnectError::NoRecords => "geario-connect-NoRecords",
            ConnectError::Unresolved => "geario-connect-Unresolved",
            ConnectError::Io(err) => err.signature(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::redundant_clone)]
    fn connect_error_clone() {
        let _ = ConnectError::Resolver(io::Error::other("test")).clone();
        let _ = ConnectError::NoRecords.clone();
        let _ = ConnectError::InvalidInput.clone();
        let _ = ConnectError::Unresolved.clone();
        let _ = ConnectError::Io(io::Error::other("test")).clone();
    }

    #[test]
    fn error_diagnostic() {
        let err = ConnectError::InvalidInput;
        assert_eq!(err.signature(), "geario-connect-InvalidInput");

        let err = ConnectError::Resolver(io::Error::other("test"));
        assert_eq!(err.signature(), "geario-connect-Resolver");

        let err = ConnectError::NoRecords;
        assert_eq!(err.signature(), "geario-connect-NoRecords");

        let err = ConnectError::Unresolved;
        assert_eq!(err.signature(), "geario-connect-Unresolved");

        let err = ConnectError::Io(io::Error::new(io::ErrorKind::InvalidInput, "test"));
        assert_eq!(err.signature(), "io-InvalidInput");

        let err = ConnectError::Io(io::Error::other("test"));
        assert_eq!(err.signature(), "io-Error");
    }
}
