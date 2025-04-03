use redb::{Database, ReadableTable, TableDefinition};
use std::any::Any;
use std::error::Error;
use tokio::sync::{mpsc, oneshot};

// NOTE(dkorolev): This is a hack to avoid having to deal with the error type.
// NOTE(dkorolev): Could get away with `anyhow::Result<T>`, but striving for prod-readiness, hehe.
type DbResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

static GLOBALS: TableDefinition<u64, u64> = TableDefinition::new("globals");
static STRINGS: TableDefinition<u64, &str> = TableDefinition::new("strings");

const DEFAULT_REQUEST_QUEUE_SIZE: usize = 32;

pub trait DbRequestHandler: Send + Sync {
  type Response: Send + 'static;
  fn handle(&self, db: &Database) -> DbResult<Self::Response>;
}

trait GenericRequestHandler: Send + Sync {
  fn handle_generic_request(&self, db: &Database) -> DbResult<Box<dyn Any + Send>>;
}

impl<T: DbRequestHandler> GenericRequestHandler for T {
  fn handle_generic_request(&self, db: &Database) -> DbResult<Box<dyn Any + Send>> {
    self.handle(db).map(|v| Box::new(v) as Box<dyn Any + Send>)
  }
}

struct GenericRequest {
  request: Box<dyn GenericRequestHandler + Send + Sync>,
  response_tx: oneshot::Sender<DbResult<Box<dyn Any + Send>>>,
}

pub struct IncCounterRequest {
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

#[derive(Debug, Clone)]
pub struct IncStringRequest {
  idx: u64,
}

impl DbRequestHandler for IncStringRequest {
  type Response = String;
  fn handle(&self, db: &Database) -> DbResult<String> {
    let txn = db.begin_write()?;
    let mut new_value = String::new();
    {
      let mut table = txn.open_table(STRINGS)?;
      if let Some(value) = table.get(&self.idx)? {
        new_value = value.value().to_string();
      }
      new_value.push('.');
      table.insert(&self.idx, &new_value.as_str())?;
    }
    txn.commit()?;
    Ok(new_value)
  }
}

#[derive(Clone)]
pub struct AsyncRedb {
  request_tx: mpsc::Sender<GenericRequest>,
}

impl AsyncRedb {
  pub fn new(db: Database) -> Self {
    Self::with_queue_size(db, DEFAULT_REQUEST_QUEUE_SIZE)
  }

  pub fn with_queue_size(db: Database, queue_size: usize) -> Self {
    let (request_tx, mut request_rx): (mpsc::Sender<GenericRequest>, mpsc::Receiver<GenericRequest>) =
      mpsc::channel(queue_size);

    std::thread::spawn(move || {
      while let Some(request) = request_rx.blocking_recv() {
        let result = request.request.handle_generic_request(&db);
        let _ = request.response_tx.send(result);
      }
    });

    Self { request_tx }
  }

  pub async fn send_request<R>(&self, request: R) -> DbResult<R::Response>
  where
    R: DbRequestHandler + Send + Sync + 'static,
  {
    let (response_tx, response_rx) = oneshot::channel();

    let request = GenericRequest { request: Box::new(request), response_tx };

    self.request_tx.send(request).await?;

    let boxed_result = response_rx.await.map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)??;
    boxed_result.downcast::<R::Response>().map(|b| *b).map_err(|_| {
      Box::new(std::io::Error::new(std::io::ErrorKind::Other, "type mismatch")) as Box<dyn Error + Send + Sync>
    })
  }

  pub async fn inc_counter(&self, idx: u64) -> DbResult<u64> {
    self.send_request(IncCounterRequest { idx }).await
  }

  pub async fn inc_string(&self, idx: u64) -> DbResult<String> {
    self.send_request(IncStringRequest { idx }).await
  }
}
