#[derive(Debug)]
pub enum ClientError {
    Io(std::io::Error),
}

impl From<std::io::Error> for ClientError {
    fn from(err: std::io::Error) -> Self {
        ClientError::Io(err)
    }
}

pub type ClientResult<T> = Result<T, ClientError>;
