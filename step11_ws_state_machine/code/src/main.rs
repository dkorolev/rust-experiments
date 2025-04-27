use axum::{
  extract::ws::{Message, WebSocket},
  extract::Path,
  extract::{State, WebSocketUpgrade},
  response::IntoResponse,
  routing::get,
  serve, Router,
};
use clap::Parser;
use std::{net::SocketAddr, sync::Arc};
use tokio::{net::TcpListener, sync::mpsc};

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

async fn add_handler(
  ws: WebSocketUpgrade, Path((a, b)): Path<(i32, i32)>, State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
  ws.on_upgrade(move |socket| add_handler_ws(socket, a, b, state))
}

async fn add_handler_ws(mut socket: WebSocket, a: i32, b: i32, _state: Arc<AppState>) {
  let _ = socket.send(Message::Text((a + b).to_string().into())).await;
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
    .route("/quit", get(quit_handler))
    .with_state(app_state.clone());

  let addr = SocketAddr::from(([0, 0, 0, 0], args.port));
  let listener = TcpListener::bind(addr).await.unwrap();

  println!("rust ws state machine demo up on {addr}");

  let server = serve(listener, app);

  _ = server
    .with_graceful_shutdown(async move {
      quit_rx.recv().await;
    })
    .await;

  println!("rust ws state machine demo down");
}
