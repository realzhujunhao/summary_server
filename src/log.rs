//! Tracing configuration.
//!
//! Logging consists of two layers:  
//! 1. stdout  
//! 2. a non-blocking file writer that group each log for one day  
//!
//! ### Example log of a success sequence of requests  
//! ```
//!   2024/12/07-00:59:26  INFO  Server listening to port 8080.
//!     at src/main.rs:65 on ThreadId(1)
//!
//!   2024/12/07-00:59:26  INFO  Global states init complete.
//!     at src/main.rs:76 on ThreadId(1)
//!
//!   2024/12/07-01:00:08  INFO
//! User bb58281b-e2d3-49b4-a43a-6a1bb24a595d requests video url: https://www.youtube.com/watch?v=onhbj0Nvi9A.
//!     at src/controller.rs:149 on ThreadId(17)
//!
//!   2024/12/07-01:00:15  INFO
//! Download success for uuid: "bb58281b-e2d3-49b4-a43a-6a1bb24a595d", link: "https://www.youtube.com/watch?v=onhbj0Nvi9A".
//!     at src/controller.rs:111 on ThreadId(4)
//!
//!   2024/12/07-01:00:15  INFO
//! Launching AI model for uuid: "bb58281b-e2d3-49b4-a43a-6a1bb24a595d", link: "https://www.youtube.com/watch?v=onhbj0Nvi9A".
//!     at src/controller.rs:124 on ThreadId(4)
//!
//!   2024/12/07-01:01:22  INFO
//! AI model success for uuid: "bb58281b-e2d3-49b4-a43a-6a1bb24a595d", link: "https://www.youtube.com/watch?v=onhbj0Nvi9A".
//!     at src/controller.rs:144 on ThreadId(16)
//!
//!   2024/12/07-01:03:53  INFO
//! User bb58281b-e2d3-49b4-a43a-6a1bb24a595d obtains summary result, remove entry from task table.
//!     at src/controller.rs:197 on ThreadId(16)
//!
//! ^C  2024/12/07-01:05:05  INFO  Keyboard interrupt, shutting down...
//!     at src/main.rs:94 on ThreadId(4)
//! ```
//!
//! ### Example log of failures
//! Note that [`ClientError`][`crate::exception::ClientError`] is marked as `WARN`,  
//! while [`ServerError`][`crate::exception::ServerError`] is marked as `ERROR`.  
//! ```
//!   2024/12/07-01:38:40  INFO  Server listening to port 8080.
//!     at src/main.rs:66 on ThreadId(1)
//!
//!   2024/12/07-01:38:40  INFO  Global states init complete.
//!     at src/main.rs:77 on ThreadId(1)
//!
//!   2024/12/07-01:38:47  INFO
//! User 7b846c96-0f9d-4e97-961b-2fa80bc64741 requests video url: https://www.youtube.com/watch?v=onhbj0Nv.
//!     at src/controller.rs:164 on ThreadId(16)
//!
//!   2024/12/07-01:38:48  WARN
//! User 7b846c96-0f9d-4e97-961b-2fa80bc64741 requested a invalid video url "https://www.youtube.com/watch?v=onhbj0Nv".
//!     at src/controller.rs:105 on ThreadId(4)
//!
//!   2024/12/07-01:38:56  INFO
//! User b092e965-ec90-49a9-bec1-747635dd99b7 requests video url: https://a.b.c.
//!     at src/controller.rs:164 on ThreadId(4)
//!
//!   2024/12/07-01:38:57  WARN
//! User b092e965-ec90-49a9-bec1-747635dd99b7 requested a invalid video url "https://a.b.c".
//!     at src/controller.rs:105 on ThreadId(21)
//!
//!   2024/12/07-01:39:05  WARN
//! User 0a241e00-fd20-49af-9183-f12d88c4b attempts to download without init task.
//!     at src/controller.rs:257 on ThreadId(21)
//! ```
use std::path::Path;

use time::{
    macros::{format_description, offset},
    UtcOffset,
};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::{
    fmt::format::FmtSpan, layer::SubscriberExt, util::SubscriberInitExt, Layer,
};

/// Initialize tracing and obtain [WorkerGuard][`tracing_appender::non_blocking::WorkerGuard`].
///
/// Attempt to obtain local time zone, fallback to +9 on failure.  
/// Log is of format:  
/// ```
/// year/month/day-hour/min/sec level ThreadId(n): output
/// ```
/// Purpose of [WorkerGuard][`tracing_appender::non_blocking::WorkerGuard`] is to make sure its
/// [`Drop`][`tracing_appender::non_blocking::WorkerGuard::drop()`] is invoked on abort.  
/// ```rust
/// fn drop(&mut self) {
///     match self
///         .sender
///         .send_timeout(Msg::Shutdown, Duration::from_millis(100))
///     {
///         Ok(_) => {
///             // Attempt to wait for `Worker` to flush all messages before dropping. This happens
///             // when the `Worker` calls `recv()` on a zero-capacity channel. Use `send_timeout`
///             // so that drop is not blocked indefinitely.
///             // TODO: Make timeout configurable.
///             let _ = self.shutdown.send_timeout((), Duration::from_millis(1000));
///         }
///         Err(SendTimeoutError::Disconnected(_)) => (),
///         Err(SendTimeoutError::Timeout(e)) => println!(
///             "Failed to send shutdown signal to logging worker. Error: {:?}",
///             e
///         ),
///     }
/// }
/// ```
pub fn init_tracing(path: impl AsRef<Path>) -> tracing_appender::non_blocking::WorkerGuard {
    // from_hms only returns Ok according to its source code
    let fallback_offset = offset!(+9);
    let offset = UtcOffset::current_local_offset().unwrap_or(fallback_offset);
    let formatter = format_description!("[year]/[month]/[day]-[hour]:[minute]:[second]");
    let time = tracing_subscriber::fmt::time::OffsetTime::new(offset, formatter);

    let file_appender = tracing_appender::rolling::daily(path, "log");
    let (non_block_file_wt, guard) = tracing_appender::non_blocking(file_appender);

    let file_layer = tracing_subscriber::fmt::layer()
        .pretty()
        .with_timer(time.clone())
        .with_file(true)
        .with_line_number(true)
        .with_thread_ids(true)
        .with_span_events(FmtSpan::ACTIVE)
        .with_writer(non_block_file_wt)
        .with_ansi(false)
        .with_target(false)
        .with_filter(LevelFilter::INFO);

    let std_layer = tracing_subscriber::fmt::layer()
        .pretty()
        .with_timer(time)
        .with_file(true)
        .with_line_number(true)
        .with_thread_ids(true)
        .with_span_events(FmtSpan::ACTIVE)
        .with_target(false)
        .with_filter(LevelFilter::INFO);

    tracing_subscriber::registry()
        .with(file_layer)
        .with(std_layer)
        .init();
    guard
}
