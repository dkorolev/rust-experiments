use axum::{
  extract::ws::{Message, WebSocket},
  extract::Path,
  extract::{State, WebSocketUpgrade},
  response::IntoResponse,
  routing::get,
  serve, Router,
};
use clap::Parser;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
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
}

struct AppState {
  fsm: FiniteStateMachine,
  quit_tx: mpsc::Sender<()>,
}

#[derive(Clone)]
struct AckermannContext {
  socket: Arc<Mutex<WebSocket>>,
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

// Here is the reference implementation. We need it to be compiled into a state machine!
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
      let mut socket = ctx.socket.lock().await;
      let _ = socket.send(Message::Text(msg.into())).await;
      n + 1
    } else {
      {
        let mut socket = ctx.socket.lock().await;
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
        let mut socket = ctx.socket.lock().await;
        let _ = socket.send(Message::Text(format!("{}ack({m},{n}) = {r}", indentation).into())).await;
      }

      r
    }
  })
}

async fn ackermann_handler_ws(socket: WebSocket, m: i64, n: i64, _state: Arc<AppState>) {
  let ctx = AckermannContext { socket: Arc::new(Mutex::new(socket)) };
  let _ = async_ack(ctx, m, n, 0).await;
}

async fn delay_handler(
  ws: WebSocketUpgrade, Path((t, s)): Path<(u64, String)>, State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
  ws.on_upgrade(move |socket| delay_handler_ws(socket, t, s, state))
}

async fn delay_handler_ws(mut socket: WebSocket, t: u64, s: String, _state: Arc<AppState>) {
  tokio::time::sleep(std::time::Duration::from_millis(t)).await;
  let _ = socket.send(Message::Text(format!("Delayed by {t}ms: `{s}`.").into())).await;
}

async fn divisors_handler(
  ws: WebSocketUpgrade, Path(a): Path<u64>, State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
  ws.on_upgrade(move |socket| divisors_handler_ws(socket, a, state))
}

async fn divisors_handler_ws(mut socket: WebSocket, n: u64, _state: Arc<AppState>) {
  for i in (1..=n).rev() {
    if n % i == 0 {
      tokio::time::sleep(std::time::Duration::from_millis(10)).await;
      let _ = socket.send(Message::Text(format!("A divisor of {n} is {i}.").into())).await;
    }
  }
  let _ = socket.send(Message::Text(format!("Done for {n}!").into())).await;
}

async fn root_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
  state.fsm.the_answer.clone()
}

async fn quit_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
  let _ = state.quit_tx.send(()).await;
  "TY\n"
}

#[tokio::main]
async fn main() {
  let args = Args::parse();

  let (quit_tx, mut quit_rx) = mpsc::channel::<()>(1);

  let app_state = Arc::new(AppState { fsm: FiniteStateMachine { the_answer: String::from("magic") }, quit_tx });

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

  server.with_graceful_shutdown(shutdown).await.unwrap();

  println!("rust ws state machine demo down");
}
