use axum::{
  Router,
  extract::Path,
  extract::ws::{Message, WebSocket},
  extract::{State, WebSocketUpgrade},
  response::IntoResponse,
  routing::get,
  serve,
};
use clap::Parser;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::{
  net::TcpListener,
  sync::{Mutex, mpsc},
};

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct LogicalTimeAbsoluteMs(u64);

impl LogicalTimeAbsoluteMs {
  pub fn from_millis(millis: u64) -> Self {
    Self(millis)
  }

  pub fn as_millis(&self) -> u64 {
    self.0
  }
}

impl std::fmt::Display for LogicalTimeAbsoluteMs {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", self.0)
  }
}

impl std::ops::Add<LogicalTimeDeltaMs> for LogicalTimeAbsoluteMs {
  type Output = LogicalTimeAbsoluteMs;

  fn add(self, rhs: LogicalTimeDeltaMs) -> Self::Output {
    LogicalTimeAbsoluteMs(self.0 + rhs.as_millis())
  }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct LogicalTimeDeltaMs(u64);

impl LogicalTimeDeltaMs {
  pub fn from_millis(millis: u64) -> Self {
    Self(millis)
  }

  pub fn as_millis(&self) -> u64 {
    self.0
  }
}

impl std::fmt::Display for LogicalTimeDeltaMs {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", self.0)
  }
}

impl From<u64> for LogicalTimeDeltaMs {
  fn from(millis: u64) -> Self {
    Self::from_millis(millis)
  }
}

impl From<u64> for LogicalTimeAbsoluteMs {
  fn from(millis: u64) -> Self {
    Self::from_millis(millis)
  }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct MaroonTaskId(u64);

impl MaroonTaskId {
  pub fn from_u64(id: u64) -> Self {
    Self(id)
  }
}

impl std::fmt::Display for MaroonTaskId {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", self.0)
  }
}

impl From<u64> for MaroonTaskId {
  fn from(id: u64) -> Self {
    Self::from_u64(id)
  }
}

#[derive(Debug, Clone)]
struct NextTaskIdGenerator {
  next_task_id: u64,
}

impl NextTaskIdGenerator {
  fn new() -> Self {
    Self { next_task_id: 1 }
  }

  fn next_task_id(&mut self) -> MaroonTaskId {
    let task_id = MaroonTaskId::from_u64(self.next_task_id);
    self.next_task_id += 1;
    task_id
  }
}

#[derive(Parser)]
struct Args {
  #[arg(long, default_value = "3000")]
  port: u16,
}

trait Timer: Send + Sync + 'static {
  fn millis_since_start(&self) -> LogicalTimeAbsoluteMs;
}

struct WallTimeTimer {
  start_time: std::time::Instant,
}

impl WallTimeTimer {
  fn new() -> Self {
    Self { start_time: std::time::Instant::now() }
  }
}

impl Timer for WallTimeTimer {
  fn millis_since_start(&self) -> LogicalTimeAbsoluteMs {
    LogicalTimeAbsoluteMs::from_millis(self.start_time.elapsed().as_millis() as u64)
  }
}

trait Writer: Send + Sync + 'static {
  async fn write_text(
    &self, text: String, timestamp: Option<LogicalTimeAbsoluteMs>,
  ) -> Result<(), Box<dyn std::error::Error>>
  where
    Self: Send;
}

struct WebSocketWriter {
  sender: mpsc::Sender<String>,
  _task: tokio::task::JoinHandle<()>,
}

impl WebSocketWriter {
  fn new(socket: WebSocket) -> Self {
    let (sender, mut receiver) = mpsc::channel::<String>(100);
    let mut socket = socket;

    let task = tokio::spawn(async move {
      while let Some(text) = receiver.recv().await {
        let _ = socket.send(Message::Text(text.into())).await;
      }
    });

    Self { sender, _task: task }
  }
}

impl Writer for WebSocketWriter {
  async fn write_text(
    &self, text: String, _timestamp: Option<LogicalTimeAbsoluteMs>,
  ) -> Result<(), Box<dyn std::error::Error>> {
    self.sender.send(text).await.map_err(Box::new)?;
    Ok(())
  }
}

#[derive(Clone, Debug)]
struct FibonacciBeginParams {
  n: u64,
}

#[derive(Clone, Debug)]
struct FibonacciCalculateParams {
  n: u64,
  index: u64,
  a: u64,
  b: u64,
}

#[derive(Clone, Debug)]
struct FibonacciResultParams {
  n: u64,
  result: u64,
}

#[derive(Clone, Debug)]
struct DelayedFibonacciStepParams {
  delay_ms: LogicalTimeDeltaMs,
  n: u64,
  next_index: u64,
  a: u64,
  b: u64,
}

#[derive(Clone, Debug)]
enum TaskState {
  DelayedMessageTaskBegin(LogicalTimeAbsoluteMs, String),
  DelayedMessageTaskExecute(LogicalTimeAbsoluteMs, String),
  DivisorsTaskBegin(u64),
  DivisorsTaskIteration(u64, u64),
  DivisorsPrintAndMoveOn(u64, u64),
  FibonacciTaskBegin(FibonacciBeginParams),
  FibonacciTaskCalculate(FibonacciCalculateParams),
  FibonacciTaskResult(FibonacciResultParams),
  DelayedFibonacciStep(DelayedFibonacciStepParams),
  Completed,
}

#[cfg(not(test))]
fn format_delayed_message(sleep_ms: LogicalTimeAbsoluteMs, message: &str) -> String {
  format!("Delayed by {sleep_ms}ms: `{message}`.")
}

#[cfg(not(test))]
fn format_divisor_found(n: u64, i: u64) -> String {
  format!("A divisor of {n} is {i}.")
}

#[cfg(not(test))]
fn format_divisors_done(n: u64) -> String {
  format!("Done for {n}!")
}

#[cfg(not(test))]
fn format_fibonacci_step(n: u64, index: u64, a: u64, _b: u64) -> String {
  format!("Fibonacci({index}) for {n} iterations: {a}.")
}

#[cfg(not(test))]
fn format_fibonacci_result(n: u64, result: u64) -> String {
  format!("Fibonacci({n}) = {result}.")
}

#[cfg(test)]
fn format_delayed_message(_sleep_ms: LogicalTimeAbsoluteMs, message: &str) -> String {
  message.to_string()
}

#[cfg(test)]
fn format_divisor_found(n: u64, i: u64) -> String {
  format!("{n}%{i}==0")
}

#[cfg(test)]
fn format_divisors_done(n: u64) -> String {
  format!("{n}!")
}

#[cfg(test)]
fn format_fibonacci_step(n: u64, index: u64, a: u64, _b: u64) -> String {
  format!("fib{index}[{n}]={a}")
}

#[cfg(test)]
fn format_fibonacci_result(n: u64, result: u64) -> String {
  format!("fib{n}={result}")
}

fn global_step(state: &TaskState) -> StepResult {
  match state {
    TaskState::DelayedMessageTaskBegin(sleep_ms, message) => StepResult::FixedSleep(
      LogicalTimeDeltaMs::from_millis(sleep_ms.as_millis()),
      TaskState::DelayedMessageTaskExecute(*sleep_ms, String::from(message)),
    ),
    TaskState::DelayedMessageTaskExecute(sleep_ms, message) => {
      StepResult::WriteAnd(format_delayed_message(*sleep_ms, message), TaskState::Completed)
    }
    TaskState::DivisorsTaskBegin(n) => StepResult::ResumeInstantly(TaskState::DivisorsTaskIteration(*n, *n)),
    TaskState::DivisorsTaskIteration(n, arg_i) => {
      let mut i = *arg_i;
      while i > 0 && *n % i != 0 {
        i -= 1
      }
      if i == 0 {
        StepResult::WriteAnd(format_divisors_done(*n), TaskState::Completed)
      } else {
        StepResult::FixedSleep(LogicalTimeDeltaMs::from_millis(i * 10), TaskState::DivisorsPrintAndMoveOn(*n, i))
      }
    }
    TaskState::DivisorsPrintAndMoveOn(n, i) => {
      StepResult::WriteAnd(format_divisor_found(*n, *i), TaskState::DivisorsTaskIteration(*n, i - 1))
    }
    TaskState::FibonacciTaskBegin(FibonacciBeginParams { n }) => {
      if *n <= 1 {
        StepResult::WriteAnd(format_fibonacci_result(*n, *n), TaskState::Completed)
      } else {
        StepResult::ResumeInstantly(TaskState::FibonacciTaskCalculate(FibonacciCalculateParams {
          n: *n,
          index: 1,
          a: 0,
          b: 1,
        }))
      }
    }
    TaskState::FibonacciTaskCalculate(FibonacciCalculateParams { n, index, a, b }) => {
      if *index >= *n {
        StepResult::ResumeInstantly(TaskState::FibonacciTaskResult(FibonacciResultParams { n: *n, result: *b }))
      } else {
        let delay = 5 * index;
        StepResult::WriteAnd(
          format_fibonacci_step(*n, *index, *b, *a),
          TaskState::DelayedFibonacciStep(DelayedFibonacciStepParams {
            delay_ms: LogicalTimeDeltaMs::from_millis(delay),
            n: *n,
            next_index: *index + 1,
            a: *b,
            b: *a + *b,
          }),
        )
      }
    }
    TaskState::DelayedFibonacciStep(params) => StepResult::FixedSleep(
      params.delay_ms,
      TaskState::FibonacciTaskCalculate(FibonacciCalculateParams {
        n: params.n,
        index: params.next_index,
        a: params.a,
        b: params.b,
      }),
    ),
    TaskState::FibonacciTaskResult(FibonacciResultParams { n, result }) => {
      StepResult::WriteAnd(format_fibonacci_result(*n, *result), TaskState::Completed)
    }
    TaskState::Completed => StepResult::Completed,
  }
}

struct AppState<T: Timer, W: Writer> {
  fsm: Arc<Mutex<MaroonRuntime<W>>>,
  quit_tx: mpsc::Sender<()>,
  timer: Arc<T>,
}

impl<T: Timer, W: Writer> AppState<T, W> {
  async fn schedule(
    &self, writer: Arc<W>, state: TaskState, scheduled_timestamp: LogicalTimeAbsoluteMs, task_description: String,
  ) {
    let mut fsm = self.fsm.lock().await;
    let task_id = fsm.task_id_generator.next_task_id();

    fsm
      .active_tasks
      .insert(task_id, MaroonTask::<W> { description: task_description, state, scheduled_timestamp, writer });

    fsm.pending_operations.push(TimestampedMaroonTask { scheduled_timestamp, task_id });
  }
}

struct MaroonTask<W: Writer> {
  description: String,
  state: TaskState,
  scheduled_timestamp: LogicalTimeAbsoluteMs,
  writer: Arc<W>,
}

// NOTE(dkorolev): Just `#[derive(Clone)]` above does not do the trick.
impl<W: Writer> Clone for MaroonTask<W> {
  fn clone(self: &Self) -> Self {
    Self {
      description: String::clone(&self.description),
      state: self.state.clone(),
      scheduled_timestamp: self.scheduled_timestamp,
      writer: Arc::clone(&self.writer),
    }
  }
}

struct TimestampedMaroonTask {
  scheduled_timestamp: LogicalTimeAbsoluteMs,
  task_id: MaroonTaskId,
}

impl Eq for TimestampedMaroonTask {}

impl PartialEq for TimestampedMaroonTask {
  fn eq(&self, other: &Self) -> bool {
    self.scheduled_timestamp == other.scheduled_timestamp
  }
}

impl Ord for TimestampedMaroonTask {
  fn cmp(&self, other: &Self) -> Ordering {
    // Reversed order by design.
    other.scheduled_timestamp.cmp(&self.scheduled_timestamp)
  }
}

impl PartialOrd for TimestampedMaroonTask {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

struct MaroonRuntime<W: Writer> {
  task_id_generator: NextTaskIdGenerator,
  pending_operations: BinaryHeap<TimestampedMaroonTask>,
  active_tasks: std::collections::HashMap<MaroonTaskId, MaroonTask<W>>,
}

enum StepResult {
  Completed,
  FixedSleep(LogicalTimeDeltaMs, TaskState),
  ResumeInstantly(TaskState),
  WriteAnd(String, TaskState),
}

async fn add_handler<T: Timer>(
  ws: WebSocketUpgrade, Path((a, b)): Path<(i32, i32)>, State(state): State<Arc<AppState<T, WebSocketWriter>>>,
) -> impl IntoResponse {
  ws.on_upgrade(move |socket| add_handler_ws(socket, a, b, state))
}

async fn add_handler_ws<T: Timer>(mut socket: WebSocket, a: i32, b: i32, _state: Arc<AppState<T, WebSocketWriter>>) {
  let _ = socket.send(Message::Text(format!("{}", a + b).into())).await;
}

async fn ackermann_handler<T: Timer>(
  ws: WebSocketUpgrade, Path((a, b)): Path<(i64, i64)>, State(state): State<Arc<AppState<T, WebSocketWriter>>>,
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

async fn async_ack<W: Writer>(w: Arc<W>, m: i64, n: i64, indent: usize) -> Result<i64, Box<dyn std::error::Error>> {
  let indentation = " ".repeat(indent);
  if m == 0 {
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    w.write_text(format!("{indentation}ack({m},{n}) = {n} + 1"), None).await?;
    Ok(n + 1)
  } else {
    w.write_text(format!("{}ack({m},{n}) ...", indentation), None).await?;

    let r = match (m, n) {
      (0, n) => n + 1,
      (m, 0) => Box::pin(async_ack(Arc::clone(&w), m - 1, 1, indent + 2)).await?,
      (m, n) => {
        let inner_result = Box::pin(async_ack(Arc::clone(&w), m, n - 1, indent + 2)).await?;
        Box::pin(async_ack(Arc::clone(&w), m - 1, inner_result, indent + 2)).await?
      }
    };

    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    w.write_text(format!("{}ack({m},{n}) = {r}", indentation), None).await?;
    Ok(r)
  }
}

async fn ackermann_handler_ws<T: Timer>(socket: WebSocket, m: i64, n: i64, _state: Arc<AppState<T, WebSocketWriter>>) {
  let _ = async_ack(Arc::new(WebSocketWriter::new(socket)), m, n, 0).await;
}

async fn delay_handler<T: Timer>(
  ws: WebSocketUpgrade, Path((t, s)): Path<(u64, String)>, State(state): State<Arc<AppState<T, WebSocketWriter>>>,
) -> impl IntoResponse {
  ws.on_upgrade(move |socket| delay_handler_ws(socket, state.timer.millis_since_start(), t, s, state))
}

async fn delay_handler_ws<T: Timer>(
  socket: WebSocket, ts: LogicalTimeAbsoluteMs, t: u64, s: String, state: Arc<AppState<T, WebSocketWriter>>,
) {
  state
    .schedule(
      Arc::new(WebSocketWriter::new(socket)),
      TaskState::DelayedMessageTaskBegin(LogicalTimeAbsoluteMs::from_millis(t), String::clone(&s)),
      ts,
      format!("Delayed by {}ms: `{}`.", t, s),
    )
    .await;
}

async fn divisors_handler<T: Timer>(
  ws: WebSocketUpgrade, Path(a): Path<u64>, State(state): State<Arc<AppState<T, WebSocketWriter>>>,
) -> impl IntoResponse {
  ws.on_upgrade(move |socket| divisors_handler_ws(socket, state.timer.millis_since_start(), a, state))
}

async fn divisors_handler_ws<T: Timer>(
  socket: WebSocket, ts: LogicalTimeAbsoluteMs, n: u64, state: Arc<AppState<T, WebSocketWriter>>,
) {
  state
    .schedule(Arc::new(WebSocketWriter::new(socket)), TaskState::DivisorsTaskBegin(n), ts, format!("Divisors of {}", n))
    .await;
}

async fn fibonacci_handler<T: Timer>(
  ws: WebSocketUpgrade, Path(n): Path<u64>, State(state): State<Arc<AppState<T, WebSocketWriter>>>,
) -> impl IntoResponse {
  ws.on_upgrade(move |socket| fibonacci_handler_ws(socket, state.timer.millis_since_start(), n, state))
}

async fn fibonacci_handler_ws<T: Timer>(
  socket: WebSocket, ts: LogicalTimeAbsoluteMs, n: u64, state: Arc<AppState<T, WebSocketWriter>>,
) {
  state
    .schedule(
      Arc::new(WebSocketWriter::new(socket)),
      TaskState::FibonacciTaskBegin(FibonacciBeginParams { n }),
      ts,
      format!("Fibonacci number {n}"),
    )
    .await;
}

async fn root_handler<T: Timer, W: Writer>(_state: State<Arc<AppState<T, W>>>) -> impl IntoResponse {
  "magic"
}

async fn state_handler<T: Timer, W: Writer>(State(state): State<Arc<AppState<T, W>>>) -> impl IntoResponse {
  let active_tasks_copy = state.fsm.lock().await.active_tasks.clone();

  let mut response = String::from("Active tasks:\n");

  for (id, maroon_task) in active_tasks_copy.iter() {
    response.push_str(&format!(
      "Task ID: {}, Description: {}, State: {:?}\n",
      id, maroon_task.description, maroon_task.state
    ));
  }

  if active_tasks_copy.is_empty() {
    response = String::from("No active tasks\n");
  }

  response
}

async fn quit_handler<T: Timer, W: Writer>(State(state): State<Arc<AppState<T, W>>>) -> impl IntoResponse {
  let _ = state.quit_tx.send(()).await;
  "TY\n"
}

async fn execute_pending_operations<T: Timer, W: Writer>(mut state: Arc<AppState<T, W>>) {
  loop {
    execute_pending_operations_inner(&mut state).await;

    // NOTE(dkorolev): I will eventually rewrite this w/o busy waiting.
    tokio::time::sleep(Duration::from_millis(10)).await;
  }
}

async fn execute_pending_operations_inner<T: Timer, W: Writer>(state: &mut Arc<AppState<T, W>>) {
  loop {
    let mut fsm = state.fsm.lock().await;
    let scheduled_timestamp_cutoff: LogicalTimeAbsoluteMs = state.timer.millis_since_start();
    if let Some((task_id, scheduled_timestamp)) = {
      // The `.map()` -> `.filter()` is to not keep the `.peek()`-ed reference.
      fsm
        .pending_operations
        .peek()
        .map(|t| t.scheduled_timestamp <= scheduled_timestamp_cutoff)
        .filter(|b| *b)
        .and_then(|_| fsm.pending_operations.pop())
        .map(|t| (t.task_id, t.scheduled_timestamp))
    } {
      let maroon_task = fsm
        .active_tasks
        .get(&task_id)
        .cloned()
        .expect("The task just retrieved from `fsm.pending_operations` should exist.");

      match global_step(&maroon_task.state) {
        StepResult::Completed => {
          fsm.active_tasks.remove(&task_id);
        }
        StepResult::ResumeInstantly(new_state) => {
          let maroon_task = fsm.active_tasks.get_mut(&task_id).unwrap();
          maroon_task.state = new_state;
          fsm.pending_operations.push(TimestampedMaroonTask { scheduled_timestamp, task_id });
        }
        StepResult::FixedSleep(sleep_ms, new_state) => {
          let scheduled_timestamp = scheduled_timestamp + sleep_ms;
          let maroon_task = fsm.active_tasks.get_mut(&task_id).unwrap();
          maroon_task.state = new_state;
          maroon_task.scheduled_timestamp = scheduled_timestamp;
          fsm.pending_operations.push(TimestampedMaroonTask { scheduled_timestamp, task_id });
        }
        StepResult::WriteAnd(text, new_state) => {
          let _ = maroon_task.writer.write_text(text, Some(scheduled_timestamp)).await;
          if let Some(maroon_task) = fsm.active_tasks.get_mut(&task_id) {
            maroon_task.state = new_state;
            fsm.pending_operations.push(TimestampedMaroonTask { scheduled_timestamp, task_id });
          }
        }
      }
    } else {
      break;
    }
  }
}

#[tokio::main]
async fn main() {
  let args = Args::parse();
  let timer = Arc::new(WallTimeTimer::new());
  let (quit_tx, mut quit_rx) = mpsc::channel::<()>(1);

  let app_state = Arc::new(AppState {
    fsm: Arc::new(Mutex::new(MaroonRuntime {
      task_id_generator: NextTaskIdGenerator::new(),
      pending_operations: BinaryHeap::<TimestampedMaroonTask>::new(),
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
    .route("/fibonacci/{n}", get(fibonacci_handler))
    .route("/ack/{m}/{n}", get(ackermann_handler)) // Do try `/ack/3/4`, but not `/ack/4/*`, hehe.
    .route("/state", get(state_handler))
    .route("/quit", get(quit_handler))
    .with_state(Arc::clone(&app_state));

  let addr = SocketAddr::from(([0, 0, 0, 0], args.port));
  let listener = TcpListener::bind(addr).await.unwrap();

  println!("rust ws state machine demo up on {addr}");

  let server = serve(listener, app);

  let shutdown = async move {
    quit_rx.recv().await;
  };

  tokio::select! {
    _ = server.with_graceful_shutdown(shutdown) => {},
    _ = execute_pending_operations(Arc::clone(&app_state)) => {
      unreachable!();
    }
  }

  println!("rust ws state machine demo down");
}

#[cfg(test)]
mod tests {
  use super::*;
  struct MockTimer {
    current_time: Arc<std::sync::Mutex<LogicalTimeAbsoluteMs>>,
  }

  impl MockTimer {
    fn new(initial_time: u64) -> Self {
      Self { current_time: Arc::new(std::sync::Mutex::new(LogicalTimeAbsoluteMs::from_millis(initial_time))) }
    }

    fn set_time(&self, time_ms: u64) {
      let mut current = self.current_time.lock().unwrap();
      *current = LogicalTimeAbsoluteMs::from_millis(time_ms);
    }
  }

  impl Timer for MockTimer {
    fn millis_since_start(&self) -> LogicalTimeAbsoluteMs {
      *self.current_time.lock().unwrap()
    }
  }

  struct MockWriter<T: Timer> {
    outputs: Arc<std::sync::Mutex<Vec<String>>>,
    timer: Arc<T>,
  }

  impl<T: Timer> MockWriter<T> {
    fn new_with_timer(timer: Arc<T>) -> Self {
      Self { outputs: Arc::new(std::sync::Mutex::new(Vec::new())), timer }
    }

    fn get_outputs_as_string(&self) -> String {
      self.outputs.lock().unwrap().join(";")
    }

    fn clear_outputs(&self) {
      self.outputs.lock().unwrap().clear();
    }
  }

  impl<T: Timer> Clone for MockWriter<T> {
    fn clone(&self) -> Self {
      Self { outputs: Arc::clone(&self.outputs), timer: Arc::clone(&self.timer) }
    }
  }

  impl<T: Timer> Writer for MockWriter<T> {
    async fn write_text(
      &self, text: String, timestamp: Option<LogicalTimeAbsoluteMs>,
    ) -> Result<(), Box<dyn std::error::Error>> {
      let timer = Arc::clone(&self.timer);
      let outputs = Arc::clone(&self.outputs);
      let time_to_use = timestamp.unwrap_or_else(|| timer.millis_since_start());
      outputs.lock().unwrap().push(format!("{}ms:{text}", time_to_use.as_millis()));
      Ok(())
    }
  }

  #[tokio::test]
  async fn test_state_machine_and_execution() {
    let timer = Arc::new(MockTimer::new(0));
    let (quit_tx, _) = mpsc::channel::<()>(1);
    let mut app_state = Arc::new(AppState {
      fsm: Arc::new(Mutex::new(MaroonRuntime {
        task_id_generator: NextTaskIdGenerator::new(),
        pending_operations: Default::default(),
        active_tasks: Default::default(),
      })),
      quit_tx,
      timer: Arc::clone(&timer),
    });
    let writer = Arc::new(MockWriter::new_with_timer(Arc::clone(&timer)));

    app_state
      .schedule(
        Arc::clone(&writer),
        TaskState::DivisorsTaskBegin(12),
        LogicalTimeAbsoluteMs::from_millis(0),
        "Divisors of 12".to_string(),
      )
      .await;
    app_state
      .schedule(
        Arc::clone(&writer),
        TaskState::DelayedMessageTaskBegin(LogicalTimeAbsoluteMs::from_millis(225), "HI".to_string()),
        LogicalTimeAbsoluteMs::from_millis(0),
        "Hello".to_string(),
      )
      .await;
    app_state
      .schedule(
        Arc::clone(&writer),
        TaskState::DelayedMessageTaskBegin(LogicalTimeAbsoluteMs::from_millis(75), "BYE".to_string()),
        LogicalTimeAbsoluteMs::from_millis(200),
        "Bye".to_string(),
      )
      .await;

    timer.set_time(225);
    execute_pending_operations_inner(&mut app_state).await;

    let expected1 = vec!["120ms:12%12==0", "180ms:12%6==0", "220ms:12%4==0", "225ms:HI"].join(";");
    assert_eq!(expected1, writer.get_outputs_as_string());

    timer.set_time(1000);
    execute_pending_operations_inner(&mut app_state).await;

    let expected2 = vec![
      "120ms:12%12==0",
      "180ms:12%6==0",
      "220ms:12%4==0",
      "225ms:HI",
      "250ms:12%3==0",
      "270ms:12%2==0",
      "275ms:BYE",
      "280ms:12%1==0",
      "280ms:12!",
    ]
    .join(";");
    assert_eq!(expected2, writer.get_outputs_as_string());

    writer.clear_outputs();
    app_state
      .schedule(
        Arc::clone(&writer),
        TaskState::DivisorsTaskBegin(8),
        LogicalTimeAbsoluteMs::from_millis(10_000),
        "".to_string(),
      )
      .await;
    app_state
      .schedule(
        Arc::clone(&writer),
        TaskState::DivisorsTaskBegin(9),
        LogicalTimeAbsoluteMs::from_millis(10_001),
        "".to_string(),
      )
      .await;
    app_state
      .schedule(
        Arc::clone(&writer),
        TaskState::DivisorsTaskBegin(3),
        LogicalTimeAbsoluteMs::from_millis(10_002),
        "".to_string(),
      )
      .await;
    timer.set_time(20_000);
    execute_pending_operations_inner(&mut app_state).await;

    let expected3 = vec![
      "10032ms:3%3==0",
      "10042ms:3%1==0",
      "10042ms:3!",
      "10080ms:8%8==0",
      "10091ms:9%9==0",
      "10120ms:8%4==0",
      "10121ms:9%3==0",
      "10131ms:9%1==0",
      "10131ms:9!",
      "10140ms:8%2==0",
      "10150ms:8%1==0",
      "10150ms:8!",
    ]
    .join(";");
    assert_eq!(expected3, writer.get_outputs_as_string());
  }

  #[tokio::test]
  async fn test_fibonacci_task() {
    let timer = Arc::new(MockTimer::new(0));
    let (quit_tx, _) = mpsc::channel::<()>(1);
    let mut app_state = Arc::new(AppState {
      fsm: Arc::new(Mutex::new(MaroonRuntime {
        task_id_generator: NextTaskIdGenerator::new(),
        pending_operations: Default::default(),
        active_tasks: Default::default(),
      })),
      quit_tx,
      timer: Arc::clone(&timer),
    });
    let writer = Arc::new(MockWriter::new_with_timer(Arc::clone(&timer)));

    app_state
      .schedule(
        Arc::clone(&writer),
        TaskState::FibonacciTaskBegin(FibonacciBeginParams { n: 5 }),
        LogicalTimeAbsoluteMs::from_millis(0),
        "The fifth Fibonacci number".to_string(),
      )
      .await;

    timer.set_time(4);
    execute_pending_operations_inner(&mut app_state).await;
    assert_eq!("0ms:fib1[5]=1", writer.get_outputs_as_string());

    timer.set_time(10);
    execute_pending_operations_inner(&mut app_state).await;

    assert_eq!("0ms:fib1[5]=1;5ms:fib2[5]=1", writer.get_outputs_as_string());

    timer.set_time(100);
    execute_pending_operations_inner(&mut app_state).await;

    let expected = vec!["0ms:fib1[5]=1", "5ms:fib2[5]=1", "15ms:fib3[5]=2", "30ms:fib4[5]=3", "50ms:fib5=5"].join(";");

    assert_eq!(expected, writer.get_outputs_as_string());
  }
}
