use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("EPUB error: {0}")]
    Epub(#[from] epub::doc::DocError),

    #[error("Invalid UTF-8: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),

    #[error("{0} is not a file")]
    NotAFile(String),
}
