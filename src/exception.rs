//! Data types for client and server error.
use serde::{ser::SerializeStruct, Serialize};
use thiserror::Error;

pub type AppResult<T> = Result<T, AppError>;

/// Sum type for error.
#[derive(Error, Debug, Clone)]
pub enum AppError {
    #[error("Client error: {0}")]
    Client(#[from] ClientError),
    #[error("Server error: {0}")]
    Server(#[from] ServerError),
}

/// Errors due to server's fault.
///
/// That is, cannot recover at client.
#[derive(Error, Debug, Clone)]
pub enum ServerError {
    /// Probably port has been occupied, or permission issue.
    #[error("Listen to port {0} failed.")]
    BindPort(usize),
    /// Error related to path handling.
    #[error("Parsing {0} failed.")]
    ParsePath(String),
    /// Error during async read file
    #[error("Async read file {0} failed.")]
    ReadFile(String),
    /// It does not mean command returns with failure, but rather failed to launch at all.
    #[error("Issue command {0} failed.")]
    IssueCommand(String),
    /// Failed to compress files.
    #[error("Failed to compress files.")]
    CompressFile,
    /// Need to inspect `main()`.
    #[error("Axum serve failed.")]
    AxumServe,
    /// Either whisper or openai returns an error.
    #[error("AI model abort with failure {0}.")]
    AiModel(String),
    /// `yt-dlp` cli returns an error given a valid url.
    #[error("video download failed, cause: {0}.")]
    VideoDownload(String),
}

/// Errors due to user's fault.
///
/// That is, cannot recover at server.
#[derive(Error, Debug, Clone)]
pub enum ClientError {
    /// Either the generated files were cleared or uuid is broken.
    #[error("Attempt to query non-existing token.")]
    TokenNotExist(String),
    /// Link not accessible by server.
    #[error("The link ({0}) to video does not exist.")]
    VideoLinkNotExist(String),
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut struct_s = serializer.serialize_struct("AppError", 2)?;
        struct_s.serialize_field("success", "false")?;
        match self {
            Self::Client(e) => {
                struct_s.serialize_field("err", e)?;
            }
            Self::Server(e) => {
                struct_s.serialize_field("err", e)?;
            }
        };
        struct_s.end()
    }
}

impl Serialize for ServerError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut struct_s = serializer.serialize_struct("ServerError", 2)?;
        struct_s.serialize_field("source", "server")?;
        struct_s.serialize_field("info", &self.to_string())?;
        struct_s.end()
    }
}

impl Serialize for ClientError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut struct_s = serializer.serialize_struct("ClientError", 2)?;
        struct_s.serialize_field("source", "client")?;
        struct_s.serialize_field("info", &self.to_string())?;
        struct_s.end()
    }
}
