use axum::extract::ws::{Message, WebSocket};
use axum::{
  extract::{State, WebSocketUpgrade},
  response::IntoResponse,
  routing::get,
  serve, Router,
};
use clap::Parser;
use std::{net::SocketAddr, sync::Arc};
use tokio::{
  net::TcpListener,
  sync::{mpsc, Mutex},
};

#[derive(Parser)]
struct Args {
  #[arg(long, default_value = "3000")]
  port: u16,
}

struct SharedState {
  magic: String,
}

async fn test_ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<Mutex<SharedState>>>) -> impl IntoResponse {
  let magic = state.lock().await.magic.clone();
  ws.on_upgrade(move |socket| async move { test_ws_handler_impl(socket, magic).await })
}

async fn test_ws_handler_impl(mut socket: WebSocket, msg: String) {
  let _ = socket.send(Message::Text(msg.into())).await;
}

#[tokio::main]
async fn main() {
  let args = Args::parse();

  let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

  let shared_state = Arc::new(Mutex::new(SharedState { magic: String::from("magic") }));

  let app = Router::new()
    .route("/healthz", get(|| async { "OK\n" }))
    .route("/test_ws", get(test_ws_handler))
    .route(
      "/quit",
      get({
        let shutdown_tx = shutdown_tx.clone();
        || async move {
          let _ = shutdown_tx.send(()).await;
          "yes i am shutting down\n"
        }
      }),
    )
    .with_state(shared_state);

  let addr = SocketAddr::from(([0, 0, 0, 0], args.port));
  let listener = TcpListener::bind(addr).await.unwrap();

  println!("rust http server with websockets ready on {addr}");

  let server = serve(listener, app);

  let _ = server
    .with_graceful_shutdown(async move {
      shutdown_rx.recv().await;
    })
    .await;

  println!("rust http server with websockets down");
}
