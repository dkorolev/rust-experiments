use axum::{
  extract::ws::{Message, WebSocket},
  extract::Path,
  extract::{State, WebSocketUpgrade},
  response::IntoResponse,
  routing::get,
  serve, Router,
};
use clap::Parser;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::{
  net::TcpListener,
  sync::{mpsc, Mutex},
};

type LogicalTimeMs = u64;

#[derive(Parser)]
struct Args {
  #[arg(long, default_value = "3000")]
  port: u16,
}

trait TimerTrait: Send + Sync {
  fn millis_since_start(&self) -> LogicalTimeMs;
}

struct Timer {
  start_time: std::time::Instant,
}

impl Timer {
  fn new() -> Self {
    Self { start_time: std::time::Instant::now() }
  }
}

impl TimerTrait for Timer {
  fn millis_since_start(&self) -> LogicalTimeMs {
    self.start_time.elapsed().as_millis() as LogicalTimeMs
  }
}

trait WriterTrait: Send + Sync {
  fn write_text<'a>(&'a self, text: String) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>;
}

struct WebSocketWriter {
  socket: Arc<Mutex<WebSocket>>,
}

impl Clone for WebSocketWriter {
  fn clone(&self) -> Self {
    Self { socket: self.socket.clone() }
  }
}

impl WebSocketWriter {
  fn new(socket: WebSocket) -> Self {
    Self { socket: Arc::new(Mutex::new(socket)) }
  }
}

impl WriterTrait for WebSocketWriter {
  fn write_text<'a>(&'a self, text: String) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
    Box::pin(async move {
      let mut socket = self.socket.lock().await;
      let _ = socket.send(Message::Text(text.into())).await;
    })
  }
}

#[derive(Clone, Debug)]
enum TaskState {
  DelayedMessageTaskBegin(LogicalTimeMs, String),
  DelayedMessageTaskExecute(LogicalTimeMs, String),
  DivisorsTaskBegin(u64),
  DivisorsTaskIteration(u64, u64),
  DivisorsPrintAndMoveOn(u64, u64),
  Completed,
}

fn global_step(state: &TaskState) -> StepResult {
  match state {
    TaskState::DelayedMessageTaskBegin(sleep_ms, message) => {
      StepResult::FixedSleep(*sleep_ms, TaskState::DelayedMessageTaskExecute(*sleep_ms, message.clone()))
    }
    TaskState::DelayedMessageTaskExecute(sleep_ms, message) => {
      StepResult::WriteAnd(format!("Delayed by {}ms: `{}`.", sleep_ms, message), TaskState::Completed)
    }
    TaskState::DivisorsTaskBegin(n) => StepResult::ResumeInstantly(TaskState::DivisorsTaskIteration(*n, *n)),
    TaskState::DivisorsTaskIteration(n, arg_i) => {
      let mut i = *arg_i;
      while i > 0 && *n % i != 0 {
        i -= 1
      }
      if i == 0 {
        StepResult::WriteAnd(format!("Done for {}!", n), TaskState::Completed)
      } else {
        StepResult::FixedSleep(i * 10, TaskState::DivisorsPrintAndMoveOn(*n, i))
      }
    }
    TaskState::DivisorsPrintAndMoveOn(n, i) => {
      StepResult::WriteAnd(format!("A divisor of {} is {}.", n, i), TaskState::DivisorsTaskIteration(*n, i - 1))
    }
    TaskState::Completed => StepResult::Completed,
  }
}

struct AppState {
  fsm: Arc<Mutex<FiniteStateMachine>>,
  quit_tx: mpsc::Sender<()>,
  timer: Arc<dyn TimerTrait>,
}

impl AppState {
  async fn schedule(
    &self, writer: Arc<dyn WriterTrait>, state: TaskState, scheduled_timestamp: LogicalTimeMs, task_description: String,
  ) {
    let mut fsm = self.fsm.lock().await;
    let task_id = fsm.next_task_id;
    fsm.next_task_id += 1;

    fsm.active_tasks.insert(
      task_id,
      FiniteStateMachineTask { description: task_description, state: state, scheduled_timestamp, writer },
    );

    fsm.pending_operations.push(TaskIdWithTimestamp { scheduled_timestamp, task_id });
  }
}

#[derive(Clone)]
struct FiniteStateMachineTask {
  description: String,
  state: TaskState,
  scheduled_timestamp: LogicalTimeMs,
  writer: Arc<dyn WriterTrait>,
}

struct TaskIdWithTimestamp {
  scheduled_timestamp: LogicalTimeMs,
  task_id: u64,
}

impl Eq for TaskIdWithTimestamp {}

impl PartialEq for TaskIdWithTimestamp {
  fn eq(&self, other: &Self) -> bool {
    self.scheduled_timestamp == other.scheduled_timestamp
  }
}

impl Ord for TaskIdWithTimestamp {
  fn cmp(&self, other: &Self) -> Ordering {
    // Reversed order by design.
    other.scheduled_timestamp.cmp(&self.scheduled_timestamp)
  }
}

impl PartialOrd for TaskIdWithTimestamp {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

struct FiniteStateMachine {
  next_task_id: u64,
  pending_operations: BinaryHeap<TaskIdWithTimestamp>,
  active_tasks: std::collections::HashMap<u64, FiniteStateMachineTask>,
}

enum StepResult {
  Completed,
  FixedSleep(LogicalTimeMs, TaskState),
  ResumeInstantly(TaskState),
  WriteAnd(String, TaskState),
}

#[derive(Clone)]
struct AckermannContext {
  writer: Arc<dyn WriterTrait>,
}

impl AckermannContext {
  fn new(writer: Arc<dyn WriterTrait>) -> Self {
    Self { writer }
  }
}

async fn add_handler(
  ws: WebSocketUpgrade, Path((a, b)): Path<(i32, i32)>, State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
  ws.on_upgrade(move |socket| add_handler_ws(socket, a, b, state))
}

async fn add_handler_ws(mut socket: WebSocket, a: i32, b: i32, _state: Arc<AppState>) {
  let _ = socket.send(Message::Text(format!("{}", a + b).into())).await;
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
      ctx.writer.write_text(msg).await;
      n + 1
    } else {
      ctx.writer.write_text(format!("{}ack({m},{n}) ...", indentation)).await;

      let r = match (m, n) {
        (0, n) => n + 1,
        (m, 0) => async_ack(ctx.clone(), m - 1, 1, indent + 2).await,
        (m, n) => async_ack(ctx.clone(), m - 1, async_ack(ctx.clone(), m, n - 1, indent + 2).await, indent + 2).await,
      };

      tokio::time::sleep(std::time::Duration::from_millis(10)).await;
      ctx.writer.write_text(format!("{}ack({m},{n}) = {r}", indentation)).await;
      r
    }
  })
}

async fn ackermann_handler_ws(socket: WebSocket, m: i64, n: i64, _state: Arc<AppState>) {
  let ctx = AckermannContext::new(Arc::new(WebSocketWriter::new(socket)));
  let _ = async_ack(ctx, m, n, 0).await;
}

async fn delay_handler(
  ws: WebSocketUpgrade, Path((t, s)): Path<(LogicalTimeMs, String)>, State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
  ws.on_upgrade(move |socket| delay_handler_ws(socket, state.timer.millis_since_start(), t, s, state))
}

async fn delay_handler_ws(socket: WebSocket, ts: LogicalTimeMs, t: u64, s: String, state: Arc<AppState>) {
  state
    .schedule(
      Arc::new(WebSocketWriter::new(socket)),
      TaskState::DelayedMessageTaskBegin(t, s.clone()),
      ts,
      format!("Delayed by {}ms: `{}`.", t, s),
    )
    .await;
}

async fn divisors_handler(
  ws: WebSocketUpgrade, Path(a): Path<u64>, State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
  ws.on_upgrade(move |socket| divisors_handler_ws(socket, state.timer.millis_since_start(), a, state))
}

async fn divisors_handler_ws(socket: WebSocket, ts: LogicalTimeMs, n: u64, state: Arc<AppState>) {
  state
    .schedule(Arc::new(WebSocketWriter::new(socket)), TaskState::DivisorsTaskBegin(n), ts, format!("Divisors of {}", n))
    .await;
}

async fn root_handler(_state: State<Arc<AppState>>) -> impl IntoResponse {
  "magic"
}

async fn state_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
  let active_tasks_copy = state.fsm.lock().await.active_tasks.clone();

  let mut response = String::from("Active tasks:\n");

  for (id, task) in active_tasks_copy.iter() {
    response.push_str(&format!("Task ID: {}, Description: {}, State: {:?}\n", id, task.description, task.state));
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
    execute_pending_operations_inner(state.clone()).await;

    // NOTE(dkorolev): I will eventually rewrite this w/o busy waiting.
    tokio::time::sleep(Duration::from_millis(10)).await;
  }
}

async fn execute_pending_operations_inner(state: Arc<AppState>) {
  loop {
    let now: LogicalTimeMs = state.timer.millis_since_start();
    let task_id_to_execute = {
      let mut fsm = state.fsm.lock().await;
      if let Some(peek) = fsm.pending_operations.peek() {
        if peek.scheduled_timestamp <= now {
          if let Some(task_with_timestamp) = fsm.pending_operations.pop() {
            Some(task_with_timestamp.task_id)
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

    if let Some(task_id) = task_id_to_execute {
      let task_info = {
        let fsm = state.fsm.lock().await;
        if let Some(task) = fsm.active_tasks.get(&task_id) {
          (task.state.clone(), task.scheduled_timestamp)
        } else {
          continue;
        }
      };

      let (task_state, original_timestamp) = task_info;

      match global_step(&task_state) {
        StepResult::FixedSleep(delay_ms, new_state) => {
          let mut fsm = state.fsm.lock().await;

          if let Some(task) = fsm.active_tasks.get_mut(&task_id) {
            task.state = new_state;
            task.scheduled_timestamp = original_timestamp + delay_ms;
          }

          fsm
            .pending_operations
            .push(TaskIdWithTimestamp { scheduled_timestamp: original_timestamp + delay_ms, task_id });
        }
        StepResult::ResumeInstantly(new_state) => {
          let mut fsm = state.fsm.lock().await;

          if let Some(task) = fsm.active_tasks.get_mut(&task_id) {
            task.state = new_state;
          }

          fsm.pending_operations.push(TaskIdWithTimestamp { scheduled_timestamp: original_timestamp, task_id });
        }
        StepResult::WriteAnd(message, next_step) => {
          {
            let fsm = state.fsm.lock().await;
            if let Some(task) = fsm.active_tasks.get(&task_id) {
              task.writer.write_text(message).await;
            }
          }

          let mut fsm = state.fsm.lock().await;

          if let Some(task) = fsm.active_tasks.get_mut(&task_id) {
            task.state = next_step;
          }

          fsm.pending_operations.push(TaskIdWithTimestamp { scheduled_timestamp: original_timestamp, task_id });
        }
        StepResult::Completed => {
          let mut fsm = state.fsm.lock().await;
          fsm.active_tasks.remove(&task_id);
        }
      }
    } else if state.fsm.lock().await.pending_operations.is_empty() {
      break;
    }
  }
}

#[tokio::main]
async fn main() {
  let args = Args::parse();
  let timer: Arc<dyn TimerTrait> = Arc::new(Timer::new());
  let (quit_tx, mut quit_rx) = mpsc::channel::<()>(1);

  let app_state = Arc::new(AppState {
    fsm: Arc::new(Mutex::new(FiniteStateMachine {
      next_task_id: 0,
      pending_operations: BinaryHeap::<TaskIdWithTimestamp>::new(),
      active_tasks: std::collections::HashMap::new(),
    })),
    quit_tx,
    timer,
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
