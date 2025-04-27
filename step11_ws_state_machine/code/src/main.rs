use async_trait::async_trait;
use axum::{
  extract::ws::{Message, WebSocket},
  extract::Path,
  extract::{State, WebSocketUpgrade},
  response::IntoResponse,
  routing::get,
  serve, Router,
};
use clap::Parser;
use derivative::Derivative;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::{
  net::TcpListener,
  sync::{mpsc, Mutex},
};

#[derive(Parser)]
struct Args {
  #[arg(long, default_value = "3000")]
  port: u16,
}

struct FiniteStateMachine {
  the_answer: String,
  pending_operations: Arc<Mutex<BinaryHeap<Reverse<StateMachineAdvancer>>>>,
}

struct OutputWrapper {
  socket: Arc<Mutex<WebSocket>>,
}

impl OutputWrapper {
  fn new(socket: WebSocket) -> Self {
    Self { socket: Arc::new(Mutex::new(socket)) }
  }

  async fn write(&self, text: impl Into<axum::extract::ws::Message>) {
    let mut socket = self.socket.lock().await;
    let _ = socket.send(text.into()).await;
  }
}

#[async_trait]
trait TaskStateMachine: Send {
  async fn step(&mut self, writer: &OutputWrapper) -> Option<u64>;
}

#[derive(Clone)]
struct DelayedMessageTask {
  sleep: bool,
  t: u64,
  s: String,
}

#[async_trait]
impl TaskStateMachine for DelayedMessageTask {
  async fn step(&mut self, writer: &OutputWrapper) -> Option<u64> {
    if self.sleep {
      self.sleep = false;
      Some(self.t)
    } else {
      writer.write(Message::Text(format!("Delayed by {}ms: `{}`.", self.t, self.s).into())).await;
      None
    }
  }
}

#[derive(Clone)]
struct DivisorsIterationTask {
  n: u64,
  i: u64,
}

#[async_trait]
impl TaskStateMachine for DivisorsIterationTask {
  async fn step(&mut self, writer: &OutputWrapper) -> Option<u64> {
    while self.i > 0 && self.n % self.i != 0 {
      self.i -= 1
    }
    if self.i == 0 {
      writer.write(Message::Text(format!("Done for {}!", self.n).into())).await;
      None
    } else {
      writer.write(Message::Text(format!("A divisor of {} is {}.", self.n, self.i).into())).await;
      let delay_ms = self.i * 10;
      self.i -= 1;
      Some(delay_ms)
    }
  }
}

#[derive(Derivative)]
#[derivative(PartialEq, Eq, PartialOrd, Ord)]
struct StateMachineAdvancer {
  scheduled_timestamp: Instant,

  #[derivative(PartialEq = "ignore", PartialOrd = "ignore", Ord = "ignore")]
  task: Box<dyn TaskStateMachine>,

  #[derivative(PartialEq = "ignore", PartialOrd = "ignore", Ord = "ignore")]
  writer: Arc<OutputWrapper>,
}

impl StateMachineAdvancer {
  fn new<T: TaskStateMachine + 'static>(scheduled_timestamp: Instant, task: T, writer: Arc<OutputWrapper>) -> Self {
    Self { scheduled_timestamp, task: Box::new(task), writer }
  }

  async fn step(&mut self) -> Option<u64> {
    self.task.step(&self.writer).await
  }
}

struct AppState {
  fsm: FiniteStateMachine,
  quit_tx: mpsc::Sender<()>,
}

impl AppState {
  async fn schedule<T: TaskStateMachine + 'static>(&self, socket: WebSocket, task: T) {
    let writer = Arc::new(OutputWrapper::new(socket));
    let scheduled_timestamp = Instant::now();
    self.fsm.pending_operations.lock().await.push(Reverse(StateMachineAdvancer::new(
      scheduled_timestamp,
      task,
      writer.clone(),
    )));
  }
}

#[derive(Clone)]
struct AckermannContext {
  wrapped_socket: Arc<Mutex<WebSocket>>,
}

async fn add_handler(
  ws: WebSocketUpgrade, Path((a, b)): Path<(i32, i32)>, State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
  ws.on_upgrade(move |socket| add_handler_ws(socket, a, b, state))
}

async fn add_handler_ws(mut socket: WebSocket, a: i32, b: i32, _state: Arc<AppState>) {
  let _ = socket.send(Message::Text((a + b).to_string().into())).await;
}

async fn ackermann_handler(
  ws: WebSocketUpgrade, Path((a, b)): Path<(i64, i64)>, State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
  ws.on_upgrade(move |socket| ackermann_handler_ws(socket, a, b, state))
}

// NOTE(dkorolev): Here is the reference implementation. We need it to be compiled into a state machine!
#[allow(dead_code)]
fn ackermann(m: u64, n: u64) -> u64 {
  match (m, n) {
    (0, n) => n + 1,
    (m, 0) => ackermann(m - 1, 1),
    (m, n) => ackermann(m - 1, ackermann(m, n - 1)),
  }
}

// NOTE(dkorolev): Even though the socket is "single-threaded", we still use a `Mutex` for now, because
// the `on_upgrade` operation in `axum` for WebSocket-s assumes the execution may span thread boundaries.
fn async_ack(ctx: AckermannContext, m: i64, n: i64, indent: usize) -> Pin<Box<dyn Future<Output = i64> + Send>> {
  Box::pin(async move {
    let indentation = " ".repeat(indent);
    if m == 0 {
      tokio::time::sleep(std::time::Duration::from_millis(10)).await;
      let msg = format!("{}ack({m},{n}) = {}", indentation, n + 1);
      let mut socket = ctx.wrapped_socket.lock().await;
      let _ = socket.send(Message::Text(msg.into())).await;
      n + 1
    } else {
      {
        let mut socket = ctx.wrapped_socket.lock().await;
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let _ = socket.send(Message::Text(format!("{}ack({m},{n}) ...", indentation).into())).await;
      }

      let r = match (m, n) {
        (0, n) => n + 1,
        (m, 0) => async_ack(ctx.clone(), m - 1, 1, indent + 2).await,
        (m, n) => async_ack(ctx.clone(), m - 1, async_ack(ctx.clone(), m, n - 1, indent + 2).await, indent + 2).await,
      };

      tokio::time::sleep(std::time::Duration::from_millis(10)).await;

      {
        let mut socket = ctx.wrapped_socket.lock().await;
        let _ = socket.send(Message::Text(format!("{}ack({m},{n}) = {r}", indentation).into())).await;
      }

      r
    }
  })
}

async fn ackermann_handler_ws(socket: WebSocket, m: i64, n: i64, _state: Arc<AppState>) {
  let ctx = AckermannContext { wrapped_socket: Arc::new(Mutex::new(socket)) };
  let _ = async_ack(ctx, m, n, 0).await;
}

async fn delay_handler(
  ws: WebSocketUpgrade, Path((t, s)): Path<(u64, String)>, State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
  ws.on_upgrade(move |socket| delay_handler_ws(socket, t, s, state))
}

async fn delay_handler_ws(socket: WebSocket, t: u64, s: String, state: Arc<AppState>) {
  state.schedule(socket, DelayedMessageTask { t, s, sleep: true }).await;
}

async fn divisors_handler(
  ws: WebSocketUpgrade, Path(a): Path<u64>, State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
  ws.on_upgrade(move |socket| divisors_handler_ws(socket, a, state))
}

async fn divisors_handler_ws(socket: WebSocket, n: u64, state: Arc<AppState>) {
  state.schedule(socket, DivisorsIterationTask { n: n, i: n }).await;
}

async fn root_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
  state.fsm.the_answer.clone()
}

async fn quit_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
  let _ = state.quit_tx.send(()).await;
  "TY\n"
}

async fn execute_pending_operations(state: Arc<AppState>) {
  loop {
    let now = Instant::now();

    let mut task_to_execute = None;

    {
      let mut pending_operations = state.fsm.pending_operations.lock().await;
      if let Some(peek) = pending_operations.peek() {
        if peek.0.scheduled_timestamp <= now {
          let scheduled_timestamp = peek.0.scheduled_timestamp;
          if let Some(Reverse(task)) = pending_operations.pop() {
            task_to_execute = Some((scheduled_timestamp, task));
          }
        }
      }
    }

    if let Some((timestamp, mut task)) = task_to_execute {
      match task.step().await {
        Some(delay_ms) => {
          // Re-insert this task back into the queue, after `delay_ms`.
          task.scheduled_timestamp = timestamp + Duration::from_millis(delay_ms);
          state.fsm.pending_operations.lock().await.push(Reverse(task));
        }
        None => {
          // The task is complete, no need to reschedule, drop it.
        }
      }
    }

    // NOTE(dkorolev): I will eventually rewrite this w/o busy waiting.
    tokio::time::sleep(Duration::from_millis(10)).await;
  }
}

#[tokio::main]
async fn main() {
  let args = Args::parse();

  let (quit_tx, mut quit_rx) = mpsc::channel::<()>(1);

  let app_state = Arc::new(AppState {
    fsm: FiniteStateMachine {
      the_answer: String::from("magic"),
      pending_operations: Arc::new(Mutex::new(BinaryHeap::<Reverse<StateMachineAdvancer>>::new())),
    },
    quit_tx,
  });

  let app = Router::new()
    .route("/", get(root_handler))
    .route("/add/{a}/{b}", get(add_handler))
    .route("/delay/{t}/{s}", get(delay_handler))
    .route("/divisors/{n}", get(divisors_handler))
    .route("/ack/{m}/{n}", get(ackermann_handler)) // Do try `/ack/3/4`, but not `/ack/4/*`, hehe.
    .route("/quit", get(quit_handler))
    .with_state(app_state.clone());

  let addr = SocketAddr::from(([0, 0, 0, 0], args.port));
  let listener = TcpListener::bind(addr).await.unwrap();

  println!("rust ws state machine demo up on {addr}");

  let server = serve(listener, app);

  let shutdown = async move {
    quit_rx.recv().await;
  };

  tokio::select! {
    _ = server.with_graceful_shutdown(shutdown) => {},
    _ = execute_pending_operations(app_state.clone()) => {
      unreachable!();
    }
  }

  println!("rust ws state machine demo down");
}
