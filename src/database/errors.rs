use thiserror::Error;

#[derive(Error, Debug)]
pub enum DatabaseError {
    #[error("Database connection error: {0}")]
    ConnectionError(String),
    #[error("Query error: {0}")]
    QueryError(String),
    #[error("User not found")]
    UserNotFound,
    #[error("User not authorized")]
    UserNotAuthorized,
}
