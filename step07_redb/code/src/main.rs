use axum::{extract::State, routing::get, serve, Router};
use hyper::header::HeaderMap;
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::{error::Error, fs, net::SocketAddr, path::Path};
use tokio::signal::unix::{signal, SignalKind};
use tokio::{net::TcpListener, sync::mpsc};

mod db;
mod lib {
  pub mod http;
}
use crate::{
  db::{AsyncRedb, DbRequestHandler, DbResult},
  lib::http,
};

#[repr(transparent)]
struct CounterType(bool);

impl CounterType {
  const LAUNCH: Self = Self(true);
  const ENDPOINT: Self = Self(false);
}

static COUNTERS: TableDefinition<bool, u64> = TableDefinition::new("counters");
static STRINGS: TableDefinition<u64, &str> = TableDefinition::new("strings");

struct IncCounterRequest {
  counter_type: CounterType,
}

impl DbRequestHandler for IncCounterRequest {
  type Response = u64;
  fn handle(&self, db: &Database) -> DbResult<Self::Response> {
    let write_txn = db.begin_write()?;
    let new_value;
    {
      let mut table = write_txn.open_table(COUNTERS)?;
      let current = table.get(&self.counter_type.0)?.map(|v| v.value()).unwrap_or(0);
      new_value = current + 1;
      table.insert(&self.counter_type.0, new_value)?;
    }
    write_txn.commit()?;
    Ok(new_value)
  }
}

struct IncStringRequest {
  idx: u64,
}

impl DbRequestHandler for IncStringRequest {
  type Response = String;
  fn handle(&self, db: &Database) -> DbResult<Self::Response> {
    let write_txn = db.begin_write()?;
    let new_value;
    {
      let mut table = write_txn.open_table(STRINGS)?;
      let current = table.get(self.idx)?.map(|v| v.value().to_string()).unwrap_or_default();
      new_value = format!("{}{}", current, ".");
      table.insert(self.idx, new_value.as_str())?;
    }
    write_txn.commit()?;
    Ok(new_value)
  }
}

trait AsyncRedbExt {
  async fn inc_counter(&self, counter_type: CounterType) -> DbResult<u64>;
  async fn inc_string(&self, idx: u64) -> DbResult<String>;
}

impl AsyncRedbExt for AsyncRedb {
  async fn inc_counter(&self, counter_type: CounterType) -> DbResult<u64> {
    self.send_request(IncCounterRequest { counter_type }).await
  }

  async fn inc_string(&self, idx: u64) -> DbResult<String> {
    self.send_request(IncStringRequest { idx }).await
  }
}

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
  StringCounter { value: String },
}

async fn health_handler() -> String {
  "OK\n".into()
}

async fn hello_handler() -> String {
  "hello this is a rust http server\n".into()
}

async fn quit_handler(State(state): State<AppState>) -> String {
  let _ = state.shutdown_tx.send(()).await;
  "yes i am shutting down\n".into()
}

async fn json_handler(State(state): State<AppState>, headers: HeaderMap) -> impl axum::response::IntoResponse {
  let counter_requests = state.redb.inc_counter(CounterType::ENDPOINT).await.unwrap_or(0);
  let response = JSONResponse::Counters { counter_runs: state.counter_runs, counter_requests };
  let json_string = serde_json::to_string(&response).unwrap();
  http::json_or_html(headers, &json_string).await
}

async fn string_handler(State(state): State<AppState>, headers: HeaderMap) -> impl axum::response::IntoResponse {
  let value = state.redb.inc_string(1).await.unwrap_or_default();
  let response = JSONResponse::StringCounter { value };
  let json_string = serde_json::to_string(&response).unwrap();
  http::json_or_html(headers, &json_string).await
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
  fs::create_dir_all(&Path::new("./.db"))?;
  let db = Database::create("./.db/demo.redb")?;
  let redb = AsyncRedb::new(db);
  let initial_counter_runs_value = redb.inc_counter(CounterType::LAUNCH).await?;

  let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
  let state = AppState { redb, counter_runs: initial_counter_runs_value, shutdown_tx: shutdown_tx.clone() };

  let app = Router::new()
    .route("/healthz", get(health_handler))
    .route("/", get(hello_handler))
    .route("/quit", get(quit_handler))
    .route("/json", get(json_handler))
    .route("/string", get(string_handler))
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
