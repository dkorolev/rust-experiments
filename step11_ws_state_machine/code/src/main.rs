use axum::{
  extract::ws::{Message, WebSocket},
  extract::Path,
  extract::{State, WebSocketUpgrade},
  response::IntoResponse,
  routing::get,
  serve, Error, Router,
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
  fn write_text<'a>(
    &'a self, text: String, timestamp: Option<LogicalTimeMs>,
  ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>>;
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
  fn write_text<'a>(
    &'a self, text: String, _timestamp: Option<LogicalTimeMs>,
  ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>> {
    Box::pin(async move {
      let mut socket = self.socket.lock().await;
      socket.send(Message::Text(text.into())).await
    })
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
  delay_ms: LogicalTimeMs,
  n: u64,
  next_index: u64,
  a: u64,
  b: u64,
}

#[derive(Clone, Debug)]
enum TaskState {
  DelayedMessageTaskBegin(LogicalTimeMs, String),
  DelayedMessageTaskExecute(LogicalTimeMs, String),
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
fn format_delayed_message(sleep_ms: LogicalTimeMs, message: &str) -> String {
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
fn format_delayed_message(_sleep_ms: LogicalTimeMs, message: &str) -> String {
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
    TaskState::DelayedMessageTaskBegin(sleep_ms, message) => {
      StepResult::FixedSleep(*sleep_ms, TaskState::DelayedMessageTaskExecute(*sleep_ms, message.clone()))
    }
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
        StepResult::FixedSleep(i * 10, TaskState::DivisorsPrintAndMoveOn(*n, i))
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
            delay_ms: delay,
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

    fsm
      .active_tasks
      .insert(task_id, FiniteStateMachineTask { description: task_description, state, scheduled_timestamp, writer });

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
fn async_ack(
  ctx: AckermannContext, m: i64, n: i64, indent: usize,
) -> Pin<Box<dyn Future<Output = Result<i64, Error>> + Send>> {
  Box::pin(async move {
    let indentation = " ".repeat(indent);
    if m == 0 {
      tokio::time::sleep(std::time::Duration::from_millis(10)).await;
      ctx.writer.write_text(format!("{indentation}ack({m},{n}) = {n} + 1"), None).await?;
      Ok(n + 1)
    } else {
      ctx.writer.write_text(format!("{}ack({m},{n}) ...", indentation), None).await?;

      let r = match (m, n) {
        (0, n) => n + 1,
        (m, 0) => async_ack(ctx.clone(), m - 1, 1, indent + 2).await?,
        (m, n) => {
          async_ack(ctx.clone(), m - 1, async_ack(ctx.clone(), m, n - 1, indent + 2).await?, indent + 2).await?
        }
      };

      tokio::time::sleep(std::time::Duration::from_millis(10)).await;
      ctx.writer.write_text(format!("{}ack({m},{n}) = {r}", indentation), None).await?;
      Ok(r)
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

async fn fibonacci_handler(
  ws: WebSocketUpgrade, Path(n): Path<u64>, State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
  ws.on_upgrade(move |socket| fibonacci_handler_ws(socket, state.timer.millis_since_start(), n, state))
}

async fn fibonacci_handler_ws(socket: WebSocket, ts: LogicalTimeMs, n: u64, state: Arc<AppState>) {
  state
    .schedule(
      Arc::new(WebSocketWriter::new(socket)),
      TaskState::FibonacciTaskBegin(FibonacciBeginParams { n }),
      ts,
      format!("Fibonacci number {n}"),
    )
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

async fn execute_pending_operations(mut state: Arc<AppState>) {
  loop {
    execute_pending_operations_inner(&mut state).await;

    // NOTE(dkorolev): I will eventually rewrite this w/o busy waiting.
    tokio::time::sleep(Duration::from_millis(10)).await;
  }
}

async fn execute_pending_operations_inner(state: &mut Arc<AppState>) {
  loop {
    let mut fsm = state.fsm.lock().await;
    let scheduled_timestamp_cutoff: LogicalTimeMs = state.timer.millis_since_start();
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
      let task = fsm
        .active_tasks
        .get(&task_id)
        .cloned()
        .expect("The task just retrieved from `fsm.pending_operations` should exist.");

      match global_step(&task.state) {
        StepResult::Completed => {
          fsm.active_tasks.remove(&task_id);
        }
        StepResult::ResumeInstantly(new_state) => {
          let task = fsm.active_tasks.get_mut(&task_id).unwrap();
          task.state = new_state;
          fsm.pending_operations.push(TaskIdWithTimestamp { scheduled_timestamp, task_id });
        }
        StepResult::FixedSleep(sleep_ms, new_state) => {
          let scheduled_timestamp = scheduled_timestamp + sleep_ms;
          let task = fsm.active_tasks.get_mut(&task_id).unwrap();
          task.state = new_state;
          task.scheduled_timestamp = scheduled_timestamp;
          fsm.pending_operations.push(TaskIdWithTimestamp { scheduled_timestamp, task_id });
        }
        StepResult::WriteAnd(text, new_state) => {
          let _ = task.writer.write_text(text, Some(scheduled_timestamp)).await;
          if let Some(task) = fsm.active_tasks.get_mut(&task_id) {
            task.state = new_state;
            fsm.pending_operations.push(TaskIdWithTimestamp { scheduled_timestamp, task_id });
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
    .route("/fibonacci/{n}", get(fibonacci_handler))
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

#[cfg(test)]
mod tests {
  use super::*;
  struct MockTimer {
    current_time: Arc<std::sync::Mutex<LogicalTimeMs>>,
  }

  impl MockTimer {
    fn new(initial_time: LogicalTimeMs) -> Self {
      Self { current_time: Arc::new(std::sync::Mutex::new(initial_time)) }
    }

    fn set_time(&self, time_ms: LogicalTimeMs) {
      let mut current = self.current_time.lock().unwrap();
      *current = time_ms;
    }
  }

  impl TimerTrait for MockTimer {
    fn millis_since_start(&self) -> LogicalTimeMs {
      *self.current_time.lock().unwrap()
    }
  }

  struct MockWriter {
    outputs: Arc<std::sync::Mutex<Vec<String>>>,
    timer: Arc<dyn TimerTrait>,
  }

  impl MockWriter {
    fn new_with_timer(timer: Arc<dyn TimerTrait>) -> Self {
      Self { outputs: Arc::new(std::sync::Mutex::new(Vec::new())), timer }
    }

    fn get_outputs_as_string(&self) -> String {
      self.outputs.lock().unwrap().join(";")
    }

    fn clear_outputs(&self) {
      self.outputs.lock().unwrap().clear();
    }
  }

  impl Clone for MockWriter {
    fn clone(&self) -> Self {
      Self { outputs: self.outputs.clone(), timer: self.timer.clone() }
    }
  }

  impl WriterTrait for MockWriter {
    fn write_text<'a>(
      &'a self, text: String, timestamp: Option<LogicalTimeMs>,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>> {
      Box::pin(async move {
        let time_to_use = timestamp.unwrap_or_else(|| self.timer.millis_since_start());
        self.outputs.lock().unwrap().push(format!("{time_to_use}ms:{text}"));
        Ok(())
      })
    }
  }

  #[tokio::test]
  async fn test_state_machine_and_execution() {
    let timer = Arc::new(MockTimer::new(0));
    let (quit_tx, _) = mpsc::channel::<()>(1);
    let mut app_state = Arc::new(AppState {
      fsm: Arc::new(Mutex::new(FiniteStateMachine {
        next_task_id: 0,
        pending_operations: Default::default(),
        active_tasks: Default::default(),
      })),
      quit_tx,
      timer: timer.clone(),
    });
    let writer = Arc::new(MockWriter::new_with_timer(timer.clone()));

    app_state.schedule(writer.clone(), TaskState::DivisorsTaskBegin(12), 0, "Divisors of 12".to_string()).await;
    app_state
      .schedule(writer.clone(), TaskState::DelayedMessageTaskBegin(225, "HI".to_string()), 0, "Hello".to_string())
      .await;
    app_state
      .schedule(writer.clone(), TaskState::DelayedMessageTaskBegin(75, "BYE".to_string()), 200, "Bye".to_string())
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
    app_state.schedule(writer.clone(), TaskState::DivisorsTaskBegin(8), 10_000, "".to_string()).await;
    app_state.schedule(writer.clone(), TaskState::DivisorsTaskBegin(9), 10_001, "".to_string()).await;
    app_state.schedule(writer.clone(), TaskState::DivisorsTaskBegin(3), 10_002, "".to_string()).await;
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
      fsm: Arc::new(Mutex::new(FiniteStateMachine {
        next_task_id: 0,
        pending_operations: Default::default(),
        active_tasks: Default::default(),
      })),
      quit_tx,
      timer: timer.clone(),
    });
    let writer = Arc::new(MockWriter::new_with_timer(timer.clone()));

    app_state
      .schedule(
        writer.clone(),
        TaskState::FibonacciTaskBegin(FibonacciBeginParams { n: 5 }),
        0,
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
