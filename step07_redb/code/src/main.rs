use axum::{
  extract::{Query, State},
  routing::{get, post},
  serve, Router,
};
use hyper::{body::Bytes, header::HeaderMap, StatusCode};
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::{error::Error, fs, net::SocketAddr, path::Path, time::SystemTime};
use tokio::signal::unix::{signal, SignalKind};
use tokio::{net::TcpListener, sync::mpsc};

mod db;
mod lib {
  pub mod http;
}
use crate::{
  db::{AsyncDbResult, AsyncRedb, DbRequestHandler},
  lib::http,
};

const DEFAULT_REQUEST_SIZE: u64 = 10;
const MAX_REQUEST_SIZE: u64 = 100;
#[repr(transparent)]
struct CounterType(bool);

impl CounterType {
  const LAUNCH: Self = Self(true);
  const ENDPOINT: Self = Self(false);
}

static COUNTERS: TableDefinition<bool, u64> = TableDefinition::new("counters");
static STRINGS: TableDefinition<u64, &str> = TableDefinition::new("strings");
static PROCESSED_IDS: TableDefinition<&str, bool> = TableDefinition::new("processed_ids");
static SUMS_JOURNAL: TableDefinition<u64, &[u8]> = TableDefinition::new("sums_journal");
static JOURNAL_META: TableDefinition<&str, u64> = TableDefinition::new("journal_meta");

struct IncCounterRequest {
  counter_type: CounterType,
}

impl DbRequestHandler for IncCounterRequest {
  type Response = u64;
  fn handle(&self, db: &Database) -> AsyncDbResult<Self::Response> {
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
  fn handle(&self, db: &Database) -> AsyncDbResult<Self::Response> {
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

struct CheckAndStoreIdRequest(String);

enum CheckAndStoreIdResponse {
  Unique,
  Duplicate,
}

impl DbRequestHandler for CheckAndStoreIdRequest {
  type Response = CheckAndStoreIdResponse;

  fn handle(&self, db: &Database) -> AsyncDbResult<Self::Response> {
    let write_txn = db.begin_write()?;
    {
      let mut table = write_txn.open_table(PROCESSED_IDS)?;
      if table.get(self.0.as_str())?.is_some() {
        return Ok(CheckAndStoreIdResponse::Duplicate);
      }
      table.insert(self.0.as_str(), true)?;
    }
    write_txn.commit()?;
    Ok(CheckAndStoreIdResponse::Unique)
  }
}

#[derive(Deserialize, Debug)]
struct SumRequest {
  id: String,
  a: i32,
  b: i32,
  c: i32,
}

#[derive(Serialize, Deserialize, Debug)]
struct JournalEntry {
  id: String,
  a: i32,
  b: i32,
  c: i32,
  timestamp_us: u64,
}

struct AddToJournalRequest(JournalEntry);

impl DbRequestHandler for AddToJournalRequest {
  type Response = ();

  fn handle(&self, db: &Database) -> AsyncDbResult<Self::Response> {
    let serialized = serde_json::to_vec(&self.0)?;

    let write_txn = db.begin_write()?;
    {
      let mut journal_meta = write_txn.open_table(JOURNAL_META)?;
      let mut journal = write_txn.open_table(SUMS_JOURNAL)?;

      let idx = journal_meta.get("idx")?.map(|v| v.value()).unwrap_or(0);
      journal.insert(idx, serialized.as_slice())?;
      journal_meta.insert("idx", idx + 1)?;
    }
    write_txn.commit()?;
    Ok(())
  }
}

struct GetJournalRequest(u64);

impl DbRequestHandler for GetJournalRequest {
  type Response = Vec<JournalEntry>;

  fn handle(&self, db: &Database) -> AsyncDbResult<Self::Response> {
    let read_txn = db.begin_read()?;
    let journal_meta = read_txn.open_table(JOURNAL_META)?;
    let journal = read_txn.open_table(SUMS_JOURNAL)?;

    let idx = journal_meta.get("idx")?.map(|v| v.value()).unwrap_or(0);

    let mut result = Vec::new();
    let n = std::cmp::min(std::cmp::min(self.0, idx), MAX_REQUEST_SIZE);

    let i0 = idx - n;

    for i in 0..n {
      if let Some(entry_bytes) = journal.get(i0 + i)? {
        if let Ok(entry) = serde_json::from_slice::<JournalEntry>(entry_bytes.value()) {
          result.push(entry);
        }
      }
    }

    Ok(result)
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
  Journal { entries: Vec<JournalEntry> },
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
  let counter_requests = state.redb.run(IncCounterRequest { counter_type: CounterType::ENDPOINT }).await.unwrap_or(0);
  let response = JSONResponse::Counters { counter_runs: state.counter_runs, counter_requests };
  let json_string = serde_json::to_string(&response).unwrap();
  http::json_or_html(headers, &json_string).await
}

async fn string_handler(State(state): State<AppState>, headers: HeaderMap) -> impl axum::response::IntoResponse {
  let value = state.redb.run(IncStringRequest { idx: 1 }).await.unwrap_or_default();
  let response = JSONResponse::StringCounter { value };
  let json_string = serde_json::to_string(&response).unwrap();
  http::json_or_html(headers, &json_string).await
}

#[derive(Deserialize)]
struct JournalQuery {
  #[serde(default = "default_request_size")]
  n: u64,
}

fn default_request_size() -> u64 {
  DEFAULT_REQUEST_SIZE
}

async fn journal_handler(
  State(state): State<AppState>, headers: HeaderMap, Query(query): Query<JournalQuery>,
) -> impl axum::response::IntoResponse {
  let entries = state.redb.run(GetJournalRequest(query.n)).await.unwrap_or_default();
  let response = JSONResponse::Journal { entries };
  let json_string = serde_json::to_string(&response).unwrap();
  http::json_or_html(headers, &json_string).await
}

async fn sums_handler(State(state): State<AppState>, body: Bytes) -> Result<&'static str, (StatusCode, String)> {
  match serde_json::from_slice::<SumRequest>(&body) {
    Ok(req) => {
      if req.id.is_empty() {
        Err((StatusCode::BAD_REQUEST, "Error: id cannot be empty.\n".to_string()))
      } else {
        match state.redb.run(CheckAndStoreIdRequest(req.id.clone())).await {
          Ok(inner_result) => match inner_result {
            CheckAndStoreIdResponse::Unique => {
              let result = req.c == req.a + req.b;
              if result {
                let _ = state
                  .redb
                  .run(AddToJournalRequest(JournalEntry {
                    id: req.id.clone(),
                    a: req.a,
                    b: req.b,
                    c: req.c,
                    timestamp_us: SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_micros() as u64,
                  }))
                  .await;
                Ok("OK\n")
              } else {
                Err((StatusCode::BAD_REQUEST, "Error: c must equal a + b.\n".to_string()))
              }
            }
            CheckAndStoreIdResponse::Duplicate => {
              Err((StatusCode::CONFLICT, "Error: Duplicate request ID.\n".to_string()))
            }
          },
          Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
        }
      }
    }
    Err(e) => Err((StatusCode::BAD_REQUEST, format!("Invalid JSON: {}.\n", e))),
  }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
  fs::create_dir_all(&Path::new("./.db"))?;
  let db = Database::create("./.db/demo.redb")?;
  let redb = AsyncRedb::new(db);
  let initial_counter_runs_value = redb.run(IncCounterRequest { counter_type: CounterType::LAUNCH }).await?;

  let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
  let state = AppState { redb, counter_runs: initial_counter_runs_value, shutdown_tx: shutdown_tx.clone() };

  let app = Router::new()
    .route("/healthz", get(health_handler))
    .route("/", get(hello_handler))
    .route("/quit", get(quit_handler))
    .route("/json", get(json_handler))
    .route("/string", get(string_handler))
    .route("/journal", get(journal_handler))
    .route("/sums", post(sums_handler))
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
