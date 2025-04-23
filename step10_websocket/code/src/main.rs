use askama::Template;
use axum::extract::ws::{Message, WebSocket};
use axum::{
  extract::{State, WebSocketUpgrade},
  response::IntoResponse,
  routing::get,
  serve, Router,
};
use chrono::Local;
use clap::Parser;
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
  net::TcpListener,
  signal::unix::{signal, SignalKind},
  sync::{mpsc, watch},
  time::interval,
};

#[derive(Parser)]
struct Args {
  #[arg(long, default_value = "3000")]
  port: u16,

  // NOTE(dkorolev): Just `/ws` is enough, although for a "clean" test some "ws://localhost:3000/ws" is better.
  #[arg(long, default_value = "/ws")]
  ws_url: String,
}

mod lib {
  pub mod ws_html;
}
use crate::lib::ws_html::WsHtmlTemplate;

async fn ws_handler(ws: WebSocketUpgrade, State(tx): State<Arc<watch::Sender<String>>>) -> impl IntoResponse {
  ws.on_upgrade(move |socket| ws_handler_impl(socket, tx.subscribe()))
}

async fn ws_handler_impl(socket: WebSocket, rx: watch::Receiver<String>) {
  let _ = ws_handler_impl_safe(socket, rx).await.map_err(|e| {
    let mut ignore_broken_pipe = false;
    let mut walk_error_chain = Some(e.as_ref());
    while let Some(err) = walk_error_chain {
      if let Some(inner) = err.downcast_ref::<std::io::Error>() {
        if inner.kind() == std::io::ErrorKind::BrokenPipe {
          ignore_broken_pipe = true;
          break;
        }
      }
      walk_error_chain = err.source();
    }
    if !ignore_broken_pipe {
      println!("websocket failure: {:#?}", e);
    }
  });
}

async fn ws_handler_impl_safe(
  mut socket: WebSocket, mut rx: watch::Receiver<String>,
) -> Result<(), Box<dyn std::error::Error>> {
  loop {
    rx.changed().await?;
    let s: String = { rx.borrow().clone() };
    socket.send(Message::Text(s.into())).await?;
  }
}

async fn test_ws_handler(ws: WebSocketUpgrade) -> impl IntoResponse {
  ws.on_upgrade(move |socket| test_ws_handler_impl(socket))
}

async fn test_ws_handler_impl(mut socket: WebSocket) {
  let _ = socket.send(Message::Text("magic".into())).await;
}

#[tokio::main]
async fn main() {
  let args = Args::parse();
  let html = (WsHtmlTemplate { ws_address: &args.ws_url }).render().expect("Wrong --ws-url.");

  let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

  let (tx, _) = watch::channel(String::from("ws starting up ..."));

  {
    let tx = tx.clone();
    tokio::spawn(async move {
      let mut ticker = interval(Duration::from_secs(1));
      loop {
        ticker.tick().await;
        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let _ = tx.send(now);
      }
    });
  }

  let shared_tx = Arc::new(tx.clone());
  let app = Router::new()
    .route("/healthz", get(|| async { "OK\n" }))
    .route("/", get(|| async { axum::response::Html(html) }))
    .route("/ws", get(ws_handler))
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
    .with_state(shared_tx);

  let addr = SocketAddr::from(([0, 0, 0, 0], args.port));
  let listener = TcpListener::bind(addr).await.unwrap();

  println!("rust http server with websockets ready on {}, ws exposed as {}", addr, args.ws_url);

  let server = serve(listener, app);

  let mut term_signal = signal(SignalKind::terminate()).expect("failed to register SIGTERM handler");
  let mut int_signal = signal(SignalKind::interrupt()).expect("failed to register SIGINT handler");

  tokio::select! {
    _ = server.with_graceful_shutdown(async move { shutdown_rx.recv().await; }) => { println! ("done"); }
    _ = tokio::signal::ctrl_c() => { println!("terminating due to Ctrl+C"); }
    _ = term_signal.recv() => { println!("terminating due to SIGTERM"); }
    _ = int_signal.recv() => { println!("terminating due to SIGINT"); }
  }

  println!("rust http server with websockets down");
}
