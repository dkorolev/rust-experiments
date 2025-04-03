use redb::{Database, ReadableTable, TableDefinition};
use std::error::Error;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

// NOTE(dkorolev): This is a hack to avoid having to deal with the error type.
// NOTE(dkorolev): Could get away with `anyhow::Result<T>`, but striving for prod-readiness, hehe.
type DbResult<T> = Result<T, Box<dyn Error + Send + Sync>>;
type DbResultAwaiter<T> = oneshot::Sender<DbResult<T>>;

static GLOBALS: TableDefinition<u64, u64> = TableDefinition::new("globals");

const DEFAULT_REQUEST_QUEUE_SIZE: usize = 32;

trait DbRequestHandler: Send + Sync {
  type Response: Send;
  fn handle(&self, db: &Database) -> DbResult<Self::Response>;
}

#[derive(Debug, Clone)]
struct IncCounterRequest {
  idx: u64,
}

impl DbRequestHandler for IncCounterRequest {
  type Response = u64;
  fn handle(&self, db: &Database) -> DbResult<u64> {
    let mut counter_value: u64 = 0;
    let txn = db.begin_write()?;
    {
      let mut table = txn.open_table(GLOBALS)?;
      if let Some(value) = table.get(&self.idx)? {
        counter_value = value.value();
      }
      counter_value += 1;
      println!("Run counter in the DB at index {}: {}", self.idx, counter_value);
      table.insert(&self.idx, &counter_value)?;
    }
    txn.commit()?;
    Ok(counter_value)
  }
}

#[derive(Clone)]
pub struct AsyncRedb(mpsc::Sender<(Arc<dyn DbRequestHandler<Response = u64> + Send + Sync>, DbResultAwaiter<u64>)>);

impl AsyncRedb {
  pub fn new(db: Database) -> Self {
    Self::with_queue_size(db, DEFAULT_REQUEST_QUEUE_SIZE)
  }

  pub fn with_queue_size(db: Database, queue_size: usize) -> Self {
    let (request_tx, mut request_rx): (mpsc::Sender<(Arc<dyn DbRequestHandler<Response = u64> + Send + Sync>, DbResultAwaiter<u64>)>, _) = mpsc::channel(queue_size);

    std::thread::spawn(move || {
      while let Some((request, response)) = request_rx.blocking_recv() {
        let _ = response.send(request.handle(&db));
      }
    });

    Self(request_tx)
  }

  pub async fn inc_counter(&self, idx: u64) -> DbResult<u64> {
    let (response_tx, response_rx) = oneshot::channel();
    self.0.send((Arc::new(IncCounterRequest { idx }), response_tx)).await?;
    response_rx.await.map_err(|e| Box::new(e))?
  }
}
