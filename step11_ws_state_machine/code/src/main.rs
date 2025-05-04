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
enum MaroonTaskState {
  Completed,
  DelayedMessageTaskBegin,
  DelayedMessageTaskExecute,
  DivisorsTaskBegin,
  DivisorsTaskIteration,
  DivisorsPrintAndMoveOn,
  FibonacciTaskBegin,
  FibonacciTaskCalculate,
  FibonacciTaskResult,
  FibonacciTaskStep,
  FactorialBegin,
  FactorialRecursion,
  FactorialRecursionPostWrite,
  FactorialRecursionPostSleep,
  FactorialReturn,
}

#[derive(Debug)]
enum MaroonTaskRuntime {
  DelayedMessage(MaroonTaskRuntimeDelayedMessage),
  Divisors(MaroonTaskRuntimeDivisors),
  Fibonacci(MaroonTaskRuntimeFibonacci),
  Factorial(MaroonTaskRuntimeFactorial),
}

#[derive(Clone, Debug)]
struct MaroonTaskRuntimeDelayedMessage {
  delay: LogicalTimeAbsoluteMs,
  message: String,
}

#[derive(Clone, Debug)]
struct MaroonTaskRuntimeDivisors {
  n: u64,
  i: u64,
}

#[derive(Clone, Debug)]
struct MaroonTaskRuntimeFibonacci {
  n: u64,
  index: u64,
  a: u64,
  b: u64,
  delay_ms: LogicalTimeDeltaMs,
}

#[derive(Clone, Debug)]
struct MaroonTaskRuntimeFactorial {
  n: u64,
  result: u64,
  next_multiplier: u64, // NOTE(dkorolev): This should eventually be on the stack.
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
  format!("Fibonacci({n}) for {index} : {a}.")
}

#[cfg(not(test))]
fn format_fibonacci_result(n: u64, result: u64) -> String {
  format!("Fibonacci[{n}] = {result}")
}

#[cfg(not(test))]
fn format_intermediate_factorial_result(n: u64, result: u64) -> String {
  format!("IntermediateFactorial({n}) = {result}.")
}

#[cfg(not(test))]
fn format_factorial_result(n: u64, result: u64) -> String {
  format!("Factorial({n}) = {result}.")
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

#[cfg(test)]
fn format_intermediate_factorial_result(n: u64, result: u64) -> String {
  format!("{n}!:{result}")
}

#[cfg(test)]
fn format_factorial_result(n: u64, result: u64) -> String {
  format!("{n}!={result}")
}

enum MaroonStepResult {
  Done,
  Next(MaroonTaskState),
  Sleep(LogicalTimeDeltaMs, MaroonTaskState),
  Write(String, MaroonTaskState),
  // NOTE(dkorolev): Go with an autogenerated set of types here? Might be legit.
  CallU64(MaroonTaskState, u64, MaroonTaskState),
  ReturnU64(u64),
}

fn global_step(state: MaroonTaskState, runtime: &mut MaroonTaskRuntime, ctx: MaroonContextValue) -> MaroonStepResult {
  match state {
    MaroonTaskState::DelayedMessageTaskBegin => {
      if let MaroonTaskRuntime::DelayedMessage(data) = runtime {
        MaroonStepResult::Sleep(
          LogicalTimeDeltaMs::from_millis(data.delay.as_millis()),
          MaroonTaskState::DelayedMessageTaskExecute,
        )
      } else {
        panic!("Runtime type mismatch for `DelayedMessageTaskBegin`.");
      }
    }
    MaroonTaskState::DelayedMessageTaskExecute => {
      if let MaroonTaskRuntime::DelayedMessage(data) = runtime {
        MaroonStepResult::Write(format_delayed_message(data.delay, &data.message), MaroonTaskState::Completed)
      } else {
        panic!("Runtime type mismatch for `DelayedMessageTaskExecute`.");
      }
    }
    MaroonTaskState::DivisorsTaskBegin => {
      if let MaroonTaskRuntime::Divisors(data) = runtime {
        data.i = data.n;
        MaroonStepResult::Next(MaroonTaskState::DivisorsTaskIteration)
      } else {
        panic!("Runtime type mismatch for `DivisorsTaskBegin`.");
      }
    }
    MaroonTaskState::DivisorsTaskIteration => {
      if let MaroonTaskRuntime::Divisors(data) = runtime {
        let mut i = data.i;
        while i > 0 && data.n % i != 0 {
          i -= 1;
        }
        if i == 0 {
          MaroonStepResult::Write(format_divisors_done(data.n), MaroonTaskState::Completed)
        } else {
          data.i = i;
          MaroonStepResult::Sleep(LogicalTimeDeltaMs::from_millis(i * 10), MaroonTaskState::DivisorsPrintAndMoveOn)
        }
      } else {
        panic!("Runtime type mismatch for `DivisorsTaskIteration`.");
      }
    }
    MaroonTaskState::DivisorsPrintAndMoveOn => {
      if let MaroonTaskRuntime::Divisors(data) = runtime {
        let result =
          MaroonStepResult::Write(format_divisor_found(data.n, data.i), MaroonTaskState::DivisorsTaskIteration);
        data.i -= 1;
        result
      } else {
        panic!("Runtime type mismatch for `DivisorsPrintAndMoveOn`.");
      }
    }
    MaroonTaskState::FibonacciTaskBegin => {
      if let MaroonTaskRuntime::Fibonacci(data) = runtime {
        if data.n <= 1 {
          MaroonStepResult::Write(format_fibonacci_result(data.n, data.n), MaroonTaskState::Completed)
        } else {
          data.index = 1;
          data.a = 0;
          data.b = 1;
          MaroonStepResult::Next(MaroonTaskState::FibonacciTaskCalculate)
        }
      } else {
        panic!("Runtime type mismatch for `FibonacciTaskBegin`.");
      }
    }
    MaroonTaskState::FibonacciTaskCalculate => {
      if let MaroonTaskRuntime::Fibonacci(data) = runtime {
        if data.index >= data.n {
          MaroonStepResult::Next(MaroonTaskState::FibonacciTaskResult)
        } else {
          let delay = 5 * data.index;
          data.delay_ms = LogicalTimeDeltaMs::from_millis(delay);
          MaroonStepResult::Write(
            format_fibonacci_step(data.n, data.index, data.b, data.a),
            MaroonTaskState::FibonacciTaskStep,
          )
        }
      } else {
        panic!("Runtime type mismatch for `FibonacciTaskCalculate`.");
      }
    }
    MaroonTaskState::FibonacciTaskStep => {
      if let MaroonTaskRuntime::Fibonacci(data) = runtime {
        let next_index = data.index + 1;
        let next_a = data.b;
        let next_b = data.a + data.b;
        data.index = next_index;
        data.a = next_a;
        data.b = next_b;
        MaroonStepResult::Sleep(data.delay_ms, MaroonTaskState::FibonacciTaskCalculate)
      } else {
        panic!("Runtime type mismatch for `FibonacciTaskStep`.");
      }
    }
    MaroonTaskState::FibonacciTaskResult => {
      if let MaroonTaskRuntime::Fibonacci(data) = runtime {
        MaroonStepResult::Write(format_fibonacci_result(data.n, data.b), MaroonTaskState::Completed)
      } else {
        panic!("Runtime type mismatch for `FibonacciTaskResult`.");
      }
    }
    // Factorial is effectively: `fac(i) { i > 1 ? i * fac(i-1) : 1 }`.
    // NOTE(dkorolev): We do not have a proper "on-stack" variables though, but this is a v0.1.
    MaroonTaskState::FactorialBegin => {
      if let MaroonTaskRuntime::Factorial(data) = runtime {
        data.result = 1;
        data.next_multiplier = data.n;
        MaroonStepResult::CallU64(
          MaroonTaskState::FactorialRecursion,
          data.next_multiplier,
          MaroonTaskState::FactorialReturn,
        )
      } else {
        panic!("Runtime type mismatch for `FactorialBegin`.");
      }
    }
    MaroonTaskState::FactorialRecursion => {
      if let MaroonTaskRuntime::Factorial(data) = runtime {
        if let MaroonContextValue::U64(n) = ctx {
          data.result *= n;
          data.next_multiplier = n - 1;
          MaroonStepResult::Write(
            format_intermediate_factorial_result(data.next_multiplier, data.result),
            MaroonTaskState::FactorialRecursionPostWrite,
          )
        } else {
          panic!("Unexpected argument type in `FactorialRecursion`.");
        }
      } else {
        panic!("Runtime type mismatch for `FactorialRecursion`.");
      }
    }
    MaroonTaskState::FactorialRecursionPostWrite => {
      if let MaroonTaskRuntime::Factorial(data) = runtime {
        MaroonStepResult::Sleep(
          LogicalTimeDeltaMs::from_millis(data.next_multiplier * 50),
          MaroonTaskState::FactorialRecursionPostSleep,
        )
      } else {
        panic!("Runtime type mismatch for `FactorialRecursionPostWrite`.");
      }
    }
    MaroonTaskState::FactorialRecursionPostSleep => {
      if let MaroonTaskRuntime::Factorial(data) = runtime {
        MaroonStepResult::ReturnU64(data.next_multiplier)
      } else {
        panic!("Runtime type mismatch for `FactorialRecursionPostSleep`.");
      }
    }
    MaroonTaskState::FactorialReturn => {
      if let MaroonTaskRuntime::Factorial(data) = runtime {
        if let MaroonContextValue::U64(v) = ctx {
          if v > 1 {
            MaroonStepResult::CallU64(
              MaroonTaskState::FactorialRecursion,
              data.next_multiplier,
              MaroonTaskState::FactorialReturn,
            )
          } else {
            MaroonStepResult::Write(format_factorial_result(data.n, data.result), MaroonTaskState::Completed)
          }
        } else {
          panic!("Should have a return value in `FactorialReturn`.");
        }
      } else {
        panic!("Runtime type mismatch for `FactorialReturn`.");
      }
    }
    MaroonTaskState::Completed => MaroonStepResult::Done,
  }
}

struct AppState<T: Timer, W: Writer> {
  fsm: Arc<Mutex<MaroonRuntime<W>>>,
  quit_tx: mpsc::Sender<()>,
  timer: Arc<T>,
}

impl<T: Timer, W: Writer> AppState<T, W> {
  async fn schedule(
    &self, writer: Arc<W>, state: MaroonTaskState, runtime: MaroonTaskRuntime,
    scheduled_timestamp: LogicalTimeAbsoluteMs, task_description: String,
  ) {
    let mut fsm = self.fsm.lock().await;
    let task_id = fsm.task_id_generator.next_task_id();

    fsm
      .active_tasks
      .insert(task_id, MaroonTask { description: task_description, runtime, writer, call_stack: vec![state] });
    fsm.pending_operations.push(TimestampedMaroonTask::new(scheduled_timestamp, task_id));
  }
}

struct MaroonTask<W: Writer> {
  description: String,
  runtime: MaroonTaskRuntime,
  writer: Arc<W>,
  call_stack: Vec<MaroonTaskState>,
}

// NOTE(dkorolev): Go with an autogenerated set of types here? Might be legit.
enum MaroonContextValue {
  None, // NOTE(dkorolev): Could pass in task ID for journaling purposes, need to chat about this with the Maroon Crew!
  U64(u64),
}

struct TimestampedMaroonTask {
  scheduled_timestamp: LogicalTimeAbsoluteMs,
  task_id: MaroonTaskId,
  ctx: MaroonContextValue,
}

impl TimestampedMaroonTask {
  fn new(scheduled_timestamp: LogicalTimeAbsoluteMs, task_id: MaroonTaskId) -> Self {
    Self { scheduled_timestamp, task_id, ctx: MaroonContextValue::None }
  }
  // For function calls and `return`-s.
  // NOTE(dkorolev): This can be done better.
  fn new_with_context_value(
    scheduled_timestamp: LogicalTimeAbsoluteMs, task_id: MaroonTaskId, ctx: MaroonContextValue,
  ) -> Self {
    Self { scheduled_timestamp, task_id, ctx: ctx }
  }
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
    w.write_text(format!("{indentation}ack({m},{n}) = {}", n + 1), None).await?;
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
  let runtime = MaroonTaskRuntime::DelayedMessage(MaroonTaskRuntimeDelayedMessage {
    delay: LogicalTimeAbsoluteMs::from_millis(t),
    message: s.clone(),
  });

  state
    .schedule(
      Arc::new(WebSocketWriter::new(socket)),
      MaroonTaskState::DelayedMessageTaskBegin,
      runtime,
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
  let runtime = MaroonTaskRuntime::Divisors(MaroonTaskRuntimeDivisors { n, i: n });

  state
    .schedule(
      Arc::new(WebSocketWriter::new(socket)),
      MaroonTaskState::DivisorsTaskBegin,
      runtime,
      ts,
      format!("Divisors of {}", n),
    )
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
  let runtime = MaroonTaskRuntime::Fibonacci(MaroonTaskRuntimeFibonacci {
    n,
    index: 0,
    a: 0,
    b: 0,
    delay_ms: LogicalTimeDeltaMs::from_millis(0),
  });

  state
    .schedule(
      Arc::new(WebSocketWriter::new(socket)),
      MaroonTaskState::FibonacciTaskBegin,
      runtime,
      ts,
      format!("Fibonacci number {n}"),
    )
    .await;
}

async fn factorial_handler<T: Timer>(
  ws: WebSocketUpgrade, Path(n): Path<u64>, State(state): State<Arc<AppState<T, WebSocketWriter>>>,
) -> impl IntoResponse {
  ws.on_upgrade(move |socket| factorial_handler_ws(socket, state.timer.millis_since_start(), n, state))
}

async fn factorial_handler_ws<T: Timer>(
  socket: WebSocket, ts: LogicalTimeAbsoluteMs, n: u64, state: Arc<AppState<T, WebSocketWriter>>,
) {
  let runtime = MaroonTaskRuntime::Factorial(MaroonTaskRuntimeFactorial {
    n,
    result: 0,          // NOTE(dkorolev): Will get changed to `1` right away.
    next_multiplier: 0, // NOTE(dkorolev): To remove.
  });

  state
    .schedule(
      Arc::new(WebSocketWriter::new(socket)),
      MaroonTaskState::FactorialBegin,
      runtime,
      ts,
      format!("Factorial of {n}"),
    )
    .await;
}

async fn root_handler<T: Timer, W: Writer>(_state: State<Arc<AppState<T, W>>>) -> impl IntoResponse {
  "magic"
}

async fn state_handler<T: Timer, W: Writer>(State(state): State<Arc<AppState<T, W>>>) -> impl IntoResponse {
  let mut response = String::from("Active tasks:\n");
  let mut empty = true;

  for (id, maroon_task) in state.fsm.lock().await.active_tasks.iter() {
    empty = false;
    response.push_str(&format!(
      "Task ID: {}, Description: {}, Stack: {:?}\n",
      id, maroon_task.description, maroon_task.call_stack,
    ));
  }

  if empty {
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
    if let Some((task_id, scheduled_timestamp, ctx)) = {
      // The `.map()` -> `.filter()` is to not keep the `.peek()`-ed reference.
      fsm
        .pending_operations
        .peek()
        .map(|t| t.scheduled_timestamp <= scheduled_timestamp_cutoff)
        .filter(|b| *b)
        .and_then(|_| fsm.pending_operations.pop())
        .map(|t| (t.task_id, t.scheduled_timestamp, t.ctx))
    } {
      let mut maroon_task =
        fsm.active_tasks.remove(&task_id).expect("The task just retrieved from `fsm.pending_operations` should exist.");

      let current_state =
        maroon_task.call_stack.pop().expect("The active task should have at least one state in call stack.");
      match global_step(current_state, &mut maroon_task.runtime, ctx) {
        MaroonStepResult::Done => {
          fsm.active_tasks.remove(&task_id);
        }
        MaroonStepResult::Next(new_state) => {
          maroon_task.call_stack.push(new_state);
          fsm.active_tasks.insert(task_id, maroon_task);
          fsm.pending_operations.push(TimestampedMaroonTask::new(scheduled_timestamp, task_id));
        }
        MaroonStepResult::Sleep(sleep_ms, new_state) => {
          maroon_task.call_stack.push(new_state);
          let scheduled_timestamp = scheduled_timestamp + sleep_ms;
          fsm.active_tasks.insert(task_id, maroon_task);
          fsm.pending_operations.push(TimestampedMaroonTask::new(scheduled_timestamp, task_id));
        }
        MaroonStepResult::Write(text, new_state) => {
          let _ = maroon_task.writer.write_text(text, Some(scheduled_timestamp)).await;
          maroon_task.call_stack.push(new_state);
          fsm.active_tasks.insert(task_id, maroon_task);
          fsm.pending_operations.push(TimestampedMaroonTask::new(scheduled_timestamp, task_id));
        }
        MaroonStepResult::CallU64(call_state, argval, return_state) => {
          maroon_task.call_stack.push(return_state);
          maroon_task.call_stack.push(call_state);
          fsm.active_tasks.insert(task_id, maroon_task);
          fsm.pending_operations.push(TimestampedMaroonTask::new_with_context_value(
            scheduled_timestamp,
            task_id,
            MaroonContextValue::U64(argval),
          ));
        }
        MaroonStepResult::ReturnU64(retval) => {
          fsm.active_tasks.insert(task_id, maroon_task);
          fsm.pending_operations.push(TimestampedMaroonTask::new_with_context_value(
            scheduled_timestamp,
            task_id,
            MaroonContextValue::U64(retval),
          ));
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
    .route("/factorial/{n}", get(factorial_handler))
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
        MaroonTaskState::DivisorsTaskBegin,
        MaroonTaskRuntime::Divisors(MaroonTaskRuntimeDivisors { n: 12, i: 12 }),
        LogicalTimeAbsoluteMs::from_millis(0),
        "Divisors of 12".to_string(),
      )
      .await;
    app_state
      .schedule(
        Arc::clone(&writer),
        MaroonTaskState::DelayedMessageTaskBegin,
        MaroonTaskRuntime::DelayedMessage(MaroonTaskRuntimeDelayedMessage {
          delay: LogicalTimeAbsoluteMs::from_millis(225),
          message: "HI".to_string(),
        }),
        LogicalTimeAbsoluteMs::from_millis(0),
        "Hello".to_string(),
      )
      .await;
    app_state
      .schedule(
        Arc::clone(&writer),
        MaroonTaskState::DelayedMessageTaskBegin,
        MaroonTaskRuntime::DelayedMessage(MaroonTaskRuntimeDelayedMessage {
          delay: LogicalTimeAbsoluteMs::from_millis(75),
          message: "BYE".to_string(),
        }),
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
        MaroonTaskState::DivisorsTaskBegin,
        MaroonTaskRuntime::Divisors(MaroonTaskRuntimeDivisors { n: 8, i: 8 }),
        LogicalTimeAbsoluteMs::from_millis(10_000),
        "".to_string(),
      )
      .await;
    app_state
      .schedule(
        Arc::clone(&writer),
        MaroonTaskState::DivisorsTaskBegin,
        MaroonTaskRuntime::Divisors(MaroonTaskRuntimeDivisors { n: 9, i: 9 }),
        LogicalTimeAbsoluteMs::from_millis(10_001),
        "".to_string(),
      )
      .await;
    app_state
      .schedule(
        Arc::clone(&writer),
        MaroonTaskState::DivisorsTaskBegin,
        MaroonTaskRuntime::Divisors(MaroonTaskRuntimeDivisors { n: 3, i: 3 }),
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
        MaroonTaskState::FibonacciTaskBegin,
        MaroonTaskRuntime::Fibonacci(MaroonTaskRuntimeFibonacci {
          n: 5,
          index: 0,
          a: 0,
          b: 0,
          delay_ms: LogicalTimeDeltaMs::from_millis(0),
        }),
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
