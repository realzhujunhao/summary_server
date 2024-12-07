//! Backend restful API for summary service  
//!
//! This server consists of only three Restful APIs:  
//! 1. `/init`: [init_summary][`controller::init_summary`].  
//! 2. `/poll`: [poll_status][`controller::poll_status`].  
//! 3. `/download`: [fetch_archive][`controller::fetch_archive`].  
//!
//! Method is `POST` for all three endpoints.
//!
//! About general API response format, see [`models::AppResp`].  
//! About exception handling, see [`ServerError`][`exception::ServerError`] and
//! [`ClientError`][`exception::ClientError`].  
//! About log output format, see [`log`].  
//!
//! ### Safety
//! - A minimum idempotency is maintained by [`init_summary`][`controller::init_summary`] controller.  
//! - APIs are stateful, but states are limited in current session. That is, uuid for `/poll` cannot
//!   servive a page refresh.  
//!
//! #### "Why not make video link the primary key, so that result can be cached and retrieved at any moment?"  
//! It will leak the information that someone else have requested a summary for a link.  
//!
//! #### "Why not make (uuid, video link) the primary key?"
//! It wouldn't help resolve the original problem, as uuid still does not survive a page refresh.  
//!   
//! #### "Why not implement authentication, and associate tasks with user account?"  
//! That would be great, but I did not have enough time. PLUS, the authentication ecosystem is  
//! immature. At the moment I wrote this, [`axum login`](https://github.com/maxcountryman/axum-login) has only
//! 655 stars. Usually people tend to implement their own request extractor.  
//!
//! ### Architecture Diagram
//! ![arch.jpg](https://zjhpub.s3.ap-northeast-2.amazonaws.com/arch.jpg)

mod controller;
mod exception;
mod log;
mod models;
use std::{
    fs,
    path::{Path, PathBuf},
    process::exit,
    sync::Arc,
};

use axum::{
    routing::{get_service, post},
    Router,
};
use clap::Parser;
use controller::{fetch_archive, init_summary, poll_status};
use exception::{AppResult, ServerError};
use log::init_tracing;
use models::{ServerState, TaskMap};
use tokio::sync::RwLock;
use tower_http::{cors::CorsLayer, services::ServeDir};

#[derive(Parser, Debug)]
struct Cli {
    #[arg(short = 'p', long = "port")]
    port: usize,
    #[arg(short = 'l', long = "log_path")]
    log_path: Option<String>,
    #[arg(short = 'w', long = "work_dir")]
    work_dir: String,
    #[arg(short = 'd', long = "doc_dir")]
    doc_dir: String,
}

fn main() {
    let cli = Cli::parse();
    let log_dir = match &cli.log_path {
        Some(path_string) => Path::new(path_string).to_path_buf(),
        None => {
            let exec_dir = std::env::current_exe()
                .expect("cannot obtain exec path, specify log path (-l) instead");
            let parent = exec_dir.parent().expect("exec has no parent");
            let mut abs_parent = parent.canonicalize().expect("cannot obtain abs path");
            abs_parent.push("backend-log");
            fs::create_dir_all(&abs_parent).expect("cannot create default log dir");
            abs_parent
        }
    };
    let _guard = init_tracing(log_dir);

    // start async tasks
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let result = run(cli).await;
        match result {
            Ok(()) => (),
            Err(e) => {
                tracing::error!("{}", e);
                exit(1);
            }
        }
    });
}

async fn run(cli: Cli) -> AppResult<()> {
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", cli.port))
        .await
        .map_err(|_| ServerError::BindPort(cli.port))?;
    tracing::info!("Server listening to port {}.", cli.port);

    let task_status = Arc::new(RwLock::new(TaskMap::new()));
    let abs_work_dir = PathBuf::from(&cli.work_dir)
        .canonicalize()
        .map_err(|_| ServerError::ParsePath(cli.work_dir))?;
    let doc_dir = PathBuf::from(&cli.doc_dir);
    let work_dir = Arc::new(abs_work_dir);
    let global_state = ServerState {
        task_status,
        work_dir,
    };
    tracing::info!("Global states init complete.");

    let doc_service = get_service(ServeDir::new(&doc_dir));

    let app = Router::new()
        .route("/init", post(init_summary))
        .route("/poll", post(poll_status))
        .route("/download", post(fetch_archive))
        .nest_service("/doc", doc_service)
        .with_state(global_state)
        .layer(CorsLayer::very_permissive());

    axum::serve(listener, app)
        .with_graceful_shutdown(graceful_shutdown())
        .await
        .map_err(|_| ServerError::AxumServe)?;
    Ok(())
}

async fn graceful_shutdown() {
    match tokio::signal::ctrl_c().await {
        Ok(()) => {
            tracing::info!("Keyboard interrupt, shutting down...");
        }
        Err(err) => {
            eprintln!("Unable to listen for shutdown signal: {}", err);
        }
    }
}
