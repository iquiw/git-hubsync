use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub struct GitError {
    msg: String,
}

impl GitError {
    pub fn new(msg: String) -> Self {
        GitError { msg }
    }
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        write!(f, "{}", self.msg)
    }
}

impl Error for GitError {}
