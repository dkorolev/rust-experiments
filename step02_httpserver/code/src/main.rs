use axum::{response::IntoResponse, routing::get, serve, Router};
use hyper::header::HeaderMap;
use std::net::SocketAddr;
use tokio::{
  net::TcpListener,
  signal::unix::{signal, SignalKind},
  sync::mpsc,
};

const SAMPLE_JSON: &str = include_str!("sample.json");

#[path = "../lib/http.rs"]
mod http;

async fn json_handler(headers: HeaderMap) -> impl IntoResponse {
  http::json_or_html(headers, SAMPLE_JSON).await
}

#[tokio::main]
async fn main() {
  let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

  let app = Router::new()
    .route("/healthz", get(|| async { "OK\n" }))
    .route("/", get(|| async { "hello this is a rust http server\n" }))
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
    .route("/json", get(json_handler));

  let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
  let listener = TcpListener::bind(addr).await.unwrap();

  println!("rust http server ready on {}", addr);

  let server = serve(listener, app);

  let mut term_signal = signal(SignalKind::terminate()).expect("failed to register SIGTERM handler");
  let mut int_signal = signal(SignalKind::interrupt()).expect("failed to register SIGINT handler");

  tokio::select! {
    _ = server.with_graceful_shutdown(async move { shutdown_rx.recv().await; }) => { println! ("done"); }
    _ = tokio::signal::ctrl_c() => { println!("terminating due to Ctrl+C"); }
    _ = term_signal.recv() => { println!("terminating due to SIGTERM"); }
    _ = int_signal.recv() => { println!("terminating due to SIGINT"); }
  }

  println!("rust http server down");
}
