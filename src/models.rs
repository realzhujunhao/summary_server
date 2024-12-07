//! Data types for http request and response.
use std::{collections::HashMap, path::PathBuf, sync::Arc};

use serde::{ser::SerializeStruct, Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::exception::AppError;

#[derive(Clone)]
pub enum TaskStatus {
    Done,
    Err(AppError),
    Download,
    Pending,
}

pub type TaskMap = HashMap<String, TaskStatus>;

#[derive(Clone)]
pub struct ServerState {
    pub task_status: Arc<RwLock<TaskMap>>,
    pub work_dir: Arc<PathBuf>,
}

#[derive(Deserialize)]
pub struct InitiateReq {
    pub url: String,
    pub uuid: String,
}

#[derive(Serialize)]
pub struct InitiateResp {
    pub uuid: String,
}

#[derive(Deserialize)]
pub struct PollStatusReq {
    pub uuid: String,
}

#[derive(Serialize)]
pub struct PollStatusResp {
    pub done: bool,
    pub stage: TaskStatus,
    pub result: Option<String>,
}

#[derive(Deserialize)]
pub struct FetchArchiveReq {
    pub uuid: String,
}

#[derive(Serialize)]
pub struct FetchArchiveResp {
    pub init: bool,
}

/// The enum every API controller returns
///
/// A response can be  
/// either `{ success: true, data: {...} }`  
/// or     `{ success: false, err: {...} }`  
/// but never having both data and err.  
/// i.e. this is not possible  
/// ` { success: bool, data: {...}, err: {...} } `  
/// ### Examples
/// ```rust
/// let data = InitiateResp { uuid: "123".into() };
/// let resp = AppResp::Success(data);
/// let serialized = serde_json::to_string(&resp).unwrap();
/// let expected = r#"{"success":true,"data":{"uuid":"123"}}"#;
/// assert_eq!(serialized, expected);
///
/// let err = AppError::Server(BindPort(80));
/// let serialized = serde_json::to_string(&err).unwrap();
/// let expected =
///     r#"{"success":"false","err":{"source":"server","info":"Listen to port 80 failed."}}"#;
/// assert_eq!(serialized, expected);
/// ```  
/// See [`Self::serialize()`]
pub enum AppResp<T>
where
    T: Serialize,
{
    Success(T),
    Exception(AppError),
}

impl<T> Serialize for AppResp<T>
where
    T: Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut struct_s = serializer.serialize_struct("AppResp", 2)?;
        match self {
            Self::Success(data) => {
                struct_s.serialize_field("success", &true)?;
                struct_s.serialize_field("data", data)?;
            }
            Self::Exception(err) => {
                struct_s.serialize_field("success", &false)?;
                struct_s.serialize_field("err", err)?;
            }
        }
        struct_s.end()
    }
}

impl Serialize for TaskStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            TaskStatus::Done => serializer.serialize_str("Done"),
            TaskStatus::Err(_) => serializer.serialize_str("Err"),
            TaskStatus::Download => serializer.serialize_str("Download"),
            TaskStatus::Pending => serializer.serialize_str("Pending"),
        }
    }
}

#[cfg(test)]
mod test {
    use super::AppResp;
    use crate::{
        exception::{AppError, ServerError::*},
        models::InitiateResp,
    };

    #[test]
    fn test_success() {
        let data = InitiateResp { uuid: "123".into() };
        let resp = AppResp::Success(data);
        let serialized = serde_json::to_string(&resp).unwrap();
        let expected = r#"{"success":true,"data":{"uuid":"123"}}"#;
        assert_eq!(serialized, expected);
    }

    #[test]
    fn test_exception() {
        let err = AppError::Server(BindPort(80));
        let serialized = serde_json::to_string(&err).unwrap();
        let expected =
            r#"{"success":"false","err":{"source":"server","info":"Listen to port 80 failed."}}"#;
        assert_eq!(serialized, expected);
    }
}

impl ServerState {
    pub async fn update_task(&self, uuid: &str, status: TaskStatus) -> Option<TaskStatus> {
        let mut guard = self.task_status.write().await;
        guard.insert(uuid.to_string(), status)
    }

    pub async fn get_task(&self, uuid: &str) -> Option<TaskStatus> {
        let guard = self.task_status.read().await;
        guard.get(uuid).cloned()
    }

    pub async fn remove_task(&self, uuid: &str) -> Option<TaskStatus> {
        let mut guard = self.task_status.write().await;
        guard.remove(uuid)
    }

    pub async fn has_task(&self, uuid: &str) -> bool {
        let guard = self.task_status.read().await;
        guard.contains_key(uuid)
    }
}
