use redb::{Database, ReadableTable, TableDefinition};
use std::error::Error;
use tokio::sync::{mpsc, oneshot};

// NOTE(dkorolev): This is a hack to avoid having to deal with the error type.
// NOTE(dkorolev): Could get away with `anyhow::Result<T>`, but striving for prod-readiness, hehe.
type DbResult<T> = Result<T, Box<dyn Error + Send + Sync>>;
type DbResultAwaiter<T> = oneshot::Sender<DbResult<T>>;

static GLOBALS: TableDefinition<u64, u64> = TableDefinition::new("globals");

const DEFAULT_REQUEST_QUEUE_SIZE: usize = 32;

#[derive(Debug)]
enum DbRequest {
  IncCounter { idx: u64, response: DbResultAwaiter<u64> },
}

#[derive(Clone)]
pub struct AsyncRedb(mpsc::Sender<DbRequest>);

impl AsyncRedb {
  pub fn new(db: Database) -> Self {
    Self::with_queue_size(db, DEFAULT_REQUEST_QUEUE_SIZE)
  }

  pub fn with_queue_size(db: Database, queue_size: usize) -> Self {
    let (request_tx, mut request_rx) = mpsc::channel(queue_size);

    std::thread::spawn(move || {
      while let Some(request) = request_rx.blocking_recv() {
        // TODO(dkorolev): Use the visitor pattern here, or whatever the Rust way to do this is.
        match request {
          DbRequest::IncCounter { idx, response } => {
            let _ = response.send(Self::handle_inc_counter(&db, idx));
          }
        }
      }
    });

    Self(request_tx)
  }

  fn handle_inc_counter(db: &Database, idx: u64) -> DbResult<u64> {
    let mut counter_value: u64 = 0;
    let txn = db.begin_write()?;
    {
      let mut table = txn.open_table(GLOBALS)?;
      if let Some(value) = table.get(&idx)? {
        counter_value = value.value();
      }
      counter_value += 1;
      println!("Run counter in the DB at index {idx}: {counter_value}");
      table.insert(&idx, &counter_value)?;
    }
    txn.commit()?;
    Ok(counter_value)
  }

  pub async fn inc_counter(&self, idx: u64) -> DbResult<u64> {
    let (response_tx, response_rx) = oneshot::channel();
    self.0.send(DbRequest::IncCounter { idx, response: response_tx }).await?;
    response_rx.await.map_err(|e| Box::new(e))?
  }
}
