//! API controllers to which the [`axum::Router`] routes.
use std::{fs::create_dir_all, path::Path, sync::Arc};

use axum::{
    body::Body,
    extract::{Json, State},
    http::{header, HeaderMap, HeaderValue},
    response::IntoResponse,
};
use serde::Serialize;
use tokio::fs::read_to_string;
use tokio_util::io;

use crate::{
    exception::{AppError, ClientError, ServerError},
    models::{
        AppResp, FetchArchiveReq, FetchArchiveResp, InitiateReq, InitiateResp, PollStatusReq,
        PollStatusResp, ServerState, TaskStatus,
    },
};
use ::uuid::Uuid;
type JsonResp<T> = Json<AppResp<T>>;

fn ok<T: Serialize>(resp: T) -> JsonResp<T> {
    Json(AppResp::Success(resp))
}

fn err<T: Serialize>(err: impl Into<AppError>) -> JsonResp<T> {
    Json(AppResp::Exception(err.into()))
}

fn task_err(err: impl Into<AppError>) -> TaskStatus {
    TaskStatus::Err(err.into())
}

/// Submit a task that may or may not complete in future.
///
/// `POST` `/init` with body:  
/// `{ url: "a valid youtube link", uuid: "" }`  
/// It guarantees to return  
/// `{ success: true, data = { uuid = "unique ID asigned to this task" } }`  
/// Returning success does not imply the task will success, failure will be indicated in subsequent poll
/// requests
pub async fn init_summary(
    State(state): State<ServerState>,
    Json(init_body): Json<InitiateReq>,
) -> JsonResp<InitiateResp> {
    let req_uuid = init_body.uuid;
    if state.has_task(&req_uuid).await {
        // no-op for re-submission
        tracing::warn!("\nUser {req_uuid} re-submits a task");
        return ok(InitiateResp { uuid: req_uuid });
    }

    let uuid = Arc::new(Uuid::new_v4().to_string());
    let url = Arc::new(init_body.url);

    // spawn task
    let uuid_copy = Arc::clone(&uuid);
    let url_copy = Arc::clone(&url);
    tokio::spawn(async move {
        let uuid = uuid_copy.clone();
        let url = url_copy;
        let user_dir = state.work_dir.join(uuid.as_ref());
        let user_dir_str = user_dir.to_str().unwrap();
        let audio_path = user_dir.join("audio.mp3");
        let audio_path_str = audio_path.to_str().unwrap();

        if create_dir_all(&user_dir).is_err() {
            tracing::error!("\nFailed to prepare user path \"{user_dir_str}\".");
            state
                .update_task(
                    &uuid,
                    task_err(ServerError::ParsePath(user_dir_str.to_string())),
                )
                .await;
            return;
        }

        state.update_task(&uuid, TaskStatus::Download).await;
        // download video from youtube
        let args = [
            "run",
            "-n",
            "server",
            "download_mp3.sh",
            &url.clone(),
            audio_path_str,
        ];
        let Ok(download_cmd) = tokio::process::Command::new("conda")
            .args(args)
            .output()
            .await
        else {
            // failed to issue command
            let command = format!("conda {}", args.join(" "));
            tracing::error!("\nFailed to issue command {command}");

            // set failure task status
            state
                .update_task(&uuid, task_err(ServerError::IssueCommand(command)))
                .await;
            return;
        };

        if !download_cmd.status.success() {
            // download failed
            let stderr = String::from_utf8_lossy(&download_cmd.stderr).to_string();
            tracing::debug!("\nDownload failed with error message: \n{stderr}");
            if is_url_problem(&stderr) {
                // invalid url
                tracing::warn!("\nUser {uuid} requested a invalid video url \"{url}\".");
                state
                    .update_task(
                        &uuid,
                        task_err(ClientError::VideoLinkNotExist(url.to_string())),
                    )
                    .await;
            } else {
                // other fault
                tracing::error!("\n`yt-dlp` throws unexpected error: \n{stderr}");
                state
                    .update_task(&uuid, task_err(ServerError::VideoDownload(stderr)))
                    .await;
            }
            return;
        }
        tracing::info!("\nDownload success for uuid: \"{uuid}\", link: \"{url}\".");

        state.update_task(&uuid, TaskStatus::Pending).await;
        // run AI model to generate
        let args = [
            "run",
            "-n",
            "server",
            "run_model.sh",
            audio_path_str,
            user_dir_str,
        ];

        tracing::info!("\nLaunching AI model for uuid: \"{uuid}\", link: \"{url}\".");
        let Ok(model_cmd) = tokio::process::Command::new("conda")
            .args(args)
            .output()
            .await
        else {
            // failed to issue command
            let command = format!("conda {}", args.join(" "));
            tracing::error!("\nFailed to issue command \"{command}\".");

            // set failure task status
            state
                .update_task(&uuid, task_err(ServerError::IssueCommand(command)))
                .await;
            return;
        };
        if !model_cmd.status.success() {
            let stderr = String::from_utf8_lossy(&download_cmd.stderr).to_string();
            tracing::error!("\nAI model failed with error message: \n{stderr}");
            // set failure task status
            state
                .update_task(&uuid, task_err(ServerError::AiModel(stderr)))
                .await;
            return;
        }
        tracing::info!("\nAI model success for uuid: \"{uuid}\", link: \"{url}\".");

        state.update_task(&uuid, TaskStatus::Done).await;
    });

    tracing::info!("\nUser {uuid} requests video url: {url}.");
    let resp = InitiateResp {
        uuid: uuid.to_string(),
    };
    ok(resp)
}

/// Query the server the status of specified task.
///
/// `POST` `/poll` with body:  
/// `{ uuid: "unique ID assigned by /init" }`  
/// It returns  
/// `{ success: true, data = { ... } }`  
/// where `data =` one of:  
/// - Your task has been completed.  
///   `{ done: true, stage: Done, result: "the summary of your video link" }`  
/// - Server is downloading your specified video.  
///   `{ done: false, stage: Download, result: null }`  
/// - Your video is under AI processing.  
///   `{ done: false, stage: Pending, result: null }`  
///
/// Or, Your task failed.  
/// - Wrong uuid.  
///   `{ success: false, err = { source: "client", info: "..." } }`  
/// - Error occured during processing.  
///   `{ success: false, err = { source: "server", info: "..." } }`  
#[axum::debug_handler]
pub async fn poll_status(
    State(state): State<ServerState>,
    Json(poll_body): Json<PollStatusReq>,
) -> JsonResp<PollStatusResp> {
    let uuid = poll_body.uuid;
    let guard = state.task_status.read().await;
    let Some(status) = guard.get(&uuid).cloned() else {
        drop(guard);
        tracing::warn!("\nUser {uuid} without a task attempts to poll.");
        return err(ClientError::TokenNotExist(uuid));
    };
    drop(guard);
    match status {
        TaskStatus::Download => ok(PollStatusResp {
            done: false,
            stage: TaskStatus::Download,
            result: None,
        }),
        TaskStatus::Pending => ok(PollStatusResp {
            done: false,
            stage: TaskStatus::Pending,
            result: None,
        }),
        TaskStatus::Done => {
            tracing::info!("\nUser {uuid} obtains summary result, remove entry from task table.");
            state.remove_task(&uuid).await;
            let user_dir = state.work_dir.join(&uuid);
            let summary_path = user_dir.join("summary.txt");
            let sum_str = summary_path.to_string_lossy().to_string();
            let Ok(content) = read_to_string(&sum_str).await else {
                tracing::error!("\nFailed to read summary result at {sum_str}.");
                return err(ServerError::ReadFile(sum_str));
            };
            ok(PollStatusResp {
                done: true,
                stage: TaskStatus::Done,
                result: Some(content),
            })
        }
        TaskStatus::Err(app_err) => {
            tracing::info!("\nUser {uuid} observes error status, remove entry from task table.");
            state.remove_task(&uuid).await;
            err(app_err.clone())
        }
    }
}

/// Poll download entire archive for diagnosis.
///
/// `POST` `/download` with body:  
/// `{ uuid: "unique ID assigned by /init" }`  
/// It returns  
/// - error if processing failed, or uuid does not exist.  
///   `{ success: false, err = { source: "client"/"server", info: "..." } }`  
/// - dummy JSON if executed for the first time (let server compress).  
/// - http response with  
///   `content-type: application/zip`  
///
/// Frontend should poll until error or `content-type = application/zip`  
pub async fn fetch_archive(
    State(state): State<ServerState>,
    Json(fetch_body): Json<FetchArchiveReq>,
) -> impl IntoResponse {
    let uuid = fetch_body.uuid;

    let user_dir = state.work_dir.join(&uuid);
    let archive_path = user_dir.join("archive.zip");
    if !user_dir.exists() {
        tracing::warn!("\nUser {uuid} attempts to download without init task.");
        let uuid_err = ClientError::TokenNotExist(uuid);
        return <Json<AppResp<FetchArchiveResp>> as IntoResponse>::into_response(err(uuid_err))
            .into_response();
    }

    let user_dir_str = user_dir.to_str().unwrap().to_string();
    let archive_path_str = archive_path.to_str().unwrap().to_string();
    if archive_path.exists() {
        tracing::info!("\nUser {uuid} downloads \"{archive_path_str}\".");
        return download_resp(archive_path_str, "archive.zip")
            .await
            .into_response();
    }
    let state = Arc::new(state);
    let state_copy = Arc::clone(&state);
    let status = state.get_task(&uuid).await;
    if let Some(TaskStatus::Err(e)) = status {
        return <Json<AppResp<FetchArchiveResp>> as IntoResponse>::into_response(err(e))
            .into_response();
    }

    let uuid_copy = uuid.clone();
    tokio::spawn(async move {
        let state = state_copy;
        let uuid = uuid_copy;
        tracing::info!("\nUser {uuid} compressing \"{archive_path_str}\".");
        let args = ["-r", &archive_path_str, "."];
        let command = format!("zip {}", args.join(" "));
        let Ok(zip_cmd) = tokio::process::Command::new("zip")
            .args(args)
            .current_dir(&user_dir_str)
            .output()
            .await
        else {
            tracing::error!("\nFailed to issue command \"{command}\".");
            state
                .update_task(&uuid, task_err(ServerError::IssueCommand(command)))
                .await;
            return;
        };
        if !zip_cmd.status.success() {
            tracing::error!("\nFailed to compress archive \"{command}\".");
            state
                .update_task(&uuid, task_err(ServerError::CompressFile))
                .await;
            return;
        }
        tracing::info!("\nUser {uuid} compressing \"{archive_path_str}\" complete.");
    });
    ok(FetchArchiveResp { init: true }).into_response()
}

async fn download_resp(path: impl AsRef<Path>, name: &str) -> impl IntoResponse {
    let Ok(file) = tokio::fs::File::open(path).await else {
        return Err(());
    };
    let stream = io::ReaderStream::new(file);
    let body = Body::from_stream(stream);
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{}\"", name)).unwrap(),
    );
    Ok((headers, body))
}

fn is_url_problem(err_msg: &str) -> bool {
    let list = [
        "is not a valid URL",
        "Failed to resolve",
        "Video unavailable",
        "Incomplete YouTube ID",
    ];
    list.iter().any(|&s| err_msg.contains(s))
}
