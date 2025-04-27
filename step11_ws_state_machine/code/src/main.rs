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
  next_task_id: u64,
  pending_operations: BinaryHeap<Reverse<StateMachineAdvancer>>,
  active_tasks: std::collections::HashMap<u64, String>,
}

struct OutputWrapper {
  socket: Arc<Mutex<WebSocket>>,
}

impl OutputWrapper {
  async fn write_impl(&self, text: impl Into<axum::extract::ws::Message>) {
    let mut socket = self.socket.lock().await;
    let _ = socket.send(text.into()).await;
  }

  async fn write(&self, text: String) {
    self.write_impl(text).await;
  }
}

enum StepResult {
  Completed,
  FixedSleep(u64, TaskState),
  ResumeInstantly(TaskState),
  WriteAnd(String, Box<TaskState>),
}

use crate::StepResult::*;

#[derive(Clone)]
enum TaskState {
  DelayedMessageTaskBegin(u64, String),
  DelayedMessageTaskExecute(u64, String),
  DivisorsTaskBegin(u64),
  DivisorsTaskIteration(u64, u64),
  Completed,
}

async fn global_step(state: &TaskState) -> StepResult {
  match state {
    TaskState::DelayedMessageTaskBegin(sleep_ms, message) => {
      FixedSleep(*sleep_ms, TaskState::DelayedMessageTaskExecute(*sleep_ms, message.clone()))
    }
    TaskState::DelayedMessageTaskExecute(sleep_ms, message) => {
      WriteAnd(format!("Delayed by {}ms: `{}`.", sleep_ms, message), Box::new(TaskState::Completed))
    }
    TaskState::DivisorsTaskBegin(n) => ResumeInstantly(TaskState::DivisorsTaskIteration(*n, *n)),
    TaskState::DivisorsTaskIteration(n, arg_i) => {
      let mut i = *arg_i;
      while i > 0 && n % i != 0 {
        i -= 1
      }
      if i == 0 {
        WriteAnd(format!("Done for {}!", n), Box::new(TaskState::Completed))
      } else {
        WriteAnd(format!("A divisor of {} is {}.", n, i), Box::new(TaskState::DivisorsTaskIteration(*n, i - 1)))
      }
    }
    TaskState::Completed => Completed,
  }
}

#[derive(Derivative)]
#[derivative(PartialEq, Eq, PartialOrd, Ord)]
struct StateMachineAdvancer {
  scheduled_timestamp: Instant,

  #[derivative(PartialEq = "ignore", PartialOrd = "ignore", Ord = "ignore")]
  state: TaskState,

  #[derivative(PartialEq = "ignore", PartialOrd = "ignore", Ord = "ignore")]
  writer: Arc<OutputWrapper>,

  #[derivative(PartialEq = "ignore", PartialOrd = "ignore", Ord = "ignore")]
  task_id: u64,
}

struct AppState {
  fsm: Arc<Mutex<FiniteStateMachine>>,
  quit_tx: mpsc::Sender<()>,
}

impl AppState {
  async fn schedule(&self, socket: WebSocket, state: TaskState, task_description: String) {
    let mut fsm = self.fsm.lock().await;
    let task_id = fsm.next_task_id;
    fsm.next_task_id += 1;

    fsm.active_tasks.insert(task_id, task_description);

    fsm.pending_operations.push(Reverse(StateMachineAdvancer {
      scheduled_timestamp: Instant::now(),
      state,
      writer: Arc::new(OutputWrapper { socket: Arc::new(Mutex::new(socket)) }),
      task_id,
    }));
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
  state
    .schedule(socket, TaskState::DelayedMessageTaskBegin(t, s.clone()), format!("Delayed by {}ms: `{}`.", t, s))
    .await;
}

async fn divisors_handler(
  ws: WebSocketUpgrade, Path(a): Path<u64>, State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
  ws.on_upgrade(move |socket| divisors_handler_ws(socket, a, state))
}

async fn divisors_handler_ws(socket: WebSocket, n: u64, state: Arc<AppState>) {
  state.schedule(socket, TaskState::DivisorsTaskBegin(n), format!("Divisors of {}", n)).await;
}

async fn root_handler(_state: State<Arc<AppState>>) -> impl IntoResponse {
  "magic"
}

async fn state_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
  let active_tasks_copy = {
    let fsm = state.fsm.lock().await;
    fsm.active_tasks.clone()
  };

  let mut response = String::from("Active tasks:\n");

  for (id, description) in active_tasks_copy.iter() {
    response.push_str(&format!("Task ID: {}, Description: {}\n", id, description));
  }

  if active_tasks_copy.is_empty() {
    response = String::from("No active tasks\n");
  }

  response
}

async fn quit_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
  let _ = state.quit_tx.send(()).await;
  "TY\n"
}

async fn execute_pending_operations(state: Arc<AppState>) {
  loop {
    let now = Instant::now();
    let task_to_execute = {
      let mut fsm = state.fsm.lock().await;
      if let Some(peek) = fsm.pending_operations.peek() {
        if peek.0.scheduled_timestamp <= now {
          if let Some(Reverse(state_machine_advancer)) = fsm.pending_operations.pop() {
            Some(state_machine_advancer)
          } else {
            None
          }
        } else {
          None
        }
      } else {
        None
      }
    };

    if let Some(mut state_machine_advancer) = task_to_execute {
      let original_timestamp = state_machine_advancer.scheduled_timestamp;
      match global_step(&state_machine_advancer.state).await {
        FixedSleep(delay_ms, new_state) => {
          state_machine_advancer.state = new_state;
          state_machine_advancer.scheduled_timestamp = original_timestamp + Duration::from_millis(delay_ms);
          let mut fsm = state.fsm.lock().await;
          fsm.pending_operations.push(Reverse(state_machine_advancer));
        }
        ResumeInstantly(new_state) => {
          state_machine_advancer.state = new_state;
          state_machine_advancer.scheduled_timestamp = original_timestamp;
          let mut fsm = state.fsm.lock().await;
          fsm.pending_operations.push(Reverse(state_machine_advancer));
        }
        WriteAnd(message, next_step) => {
          state_machine_advancer.writer.write(message).await;
          state_machine_advancer.state = *next_step;
          let mut fsm = state.fsm.lock().await;
          fsm.pending_operations.push(Reverse(state_machine_advancer));
        }
        Completed => {
          let mut fsm = state.fsm.lock().await;
          fsm.active_tasks.remove(&state_machine_advancer.task_id);
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
    fsm: Arc::new(Mutex::new(FiniteStateMachine {
      next_task_id: 0,
      pending_operations: BinaryHeap::<Reverse<StateMachineAdvancer>>::new(),
      active_tasks: std::collections::HashMap::new(),
    })),
    quit_tx,
  });

  let app = Router::new()
    .route("/", get(root_handler))
    .route("/add/{a}/{b}", get(add_handler))
    .route("/delay/{t}/{s}", get(delay_handler))
    .route("/divisors/{n}", get(divisors_handler))
    .route("/ack/{m}/{n}", get(ackermann_handler)) // Do try `/ack/3/4`, but not `/ack/4/*`, hehe.
    .route("/state", get(state_handler))
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
