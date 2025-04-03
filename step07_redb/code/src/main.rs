use axum::{extract::State, routing::get, serve, Router};
use hyper::header::HeaderMap;
use redb::Database;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs;
use std::net::SocketAddr;
use std::path::Path;
use tokio::signal::unix::{signal, SignalKind};
use tokio::{net::TcpListener, sync::mpsc};

mod db;
mod lib {
  pub mod http;
}
use crate::{db::AsyncRedb, lib::http};

#[derive(Clone)]
struct AppState {
  redb: AsyncRedb,
  counter_runs: u64,
  shutdown_tx: mpsc::Sender<()>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
enum JSONResponse {
  Point { x: i32, y: i32 },
  Message { text: String },
  Counters { counter_runs: u64, counter_requests: u64 },
}

async fn health_handler() -> &'static str {
  "OK\n"
}

async fn hello_handler() -> &'static str {
  "hello this is a rust http server\n"
}

async fn quit_handler(State(state): State<AppState>) -> &'static str {
  let _ = state.shutdown_tx.send(()).await;
  "yes i am shutting down\n"
}

async fn json_handler(State(state): State<AppState>, headers: HeaderMap) -> impl axum::response::IntoResponse {
  let counter_requests = state.redb.inc_counter(2).await.unwrap_or(0);
  let response = JSONResponse::Counters { counter_runs: state.counter_runs, counter_requests };
  let json_string = serde_json::to_string(&response).unwrap();
  http::json_or_html(headers, &json_string).await
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
  fs::create_dir_all(&Path::new("./.db"))?;
  let db = Database::create("./.db/demo.redb")?;
  let redb = AsyncRedb::new(db);
  let initial_counter_runs_value = redb.inc_counter(1).await?;

  let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
  let state = AppState { redb, counter_runs: initial_counter_runs_value, shutdown_tx: shutdown_tx.clone() };

  let app = Router::new()
    .route("/healthz", get(health_handler))
    .route("/", get(hello_handler))
    .route("/quit", get(quit_handler))
    .route("/json", get(json_handler))
    .with_state(state);

  let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
  let listener = TcpListener::bind(addr).await.unwrap();

  println!("rust http server ready on {}", addr);

  let server = serve(listener, app);

  let mut term_signal = signal(SignalKind::terminate()).expect("failed to register SIGTERM handler");
  let mut int_signal = signal(SignalKind::interrupt()).expect("failed to register SIGINT handler");

  tokio::select! {
    _ = server.with_graceful_shutdown(async move { shutdown_rx.recv().await; }) => { println!("done"); }
    _ = tokio::signal::ctrl_c() => { println!("terminating due to Ctrl+C"); }
    _ = term_signal.recv() => { println!("terminating due to SIGTERM"); }
    _ = int_signal.recv() => { println!("terminating due to SIGINT"); }
  }

  println!("rust http server down");
  Ok(())
}
