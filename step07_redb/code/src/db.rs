use redb::Database;
use std::any::Any;
use std::error::Error;
use tokio::sync::{mpsc, oneshot};

pub type DbResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

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
}
