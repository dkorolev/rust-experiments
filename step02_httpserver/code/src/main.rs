use askama::Template;
use axum::{
  response::{Html, IntoResponse, Response},
  routing::get,
  serve, Router,
};
use hyper::{
  header::{self, HeaderMap},
  StatusCode,
};
use std::net::SocketAddr;
use tokio::{
  net::TcpListener,
  signal::unix::{signal, SignalKind},
  sync::mpsc,
};

#[derive(Template)]
#[template(path = "jsontemplate.html", escape = "none")]
struct DataHtmlTemplate<'a> {
  raw_json_as_string: &'a str,
}

const SAMPLE_JSON: &str = include_str!("sample.json");

async fn json_or_html(headers: HeaderMap) -> impl IntoResponse {
  let raw_json_as_string = SAMPLE_JSON;
  if accept_header_contains_text_html(&headers) {
    let template = DataHtmlTemplate { raw_json_as_string };
    match template.render() {
      Ok(html) => Html(html).into_response(),
      Err(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
  } else {
    Response::builder()
      .status(StatusCode::OK)
      .header("content-type", "application/json")
      .body(raw_json_as_string.to_string())
      .unwrap()
      .into_response()
  }
}

fn accept_header_contains_text_html(headers: &HeaderMap) -> bool {
  headers
    .get_all(header::ACCEPT)
    .iter()
    .filter_map(|s| s.to_str().ok())
    .flat_map(|s| s.split(','))
    .map(|s| s.split(';').next().unwrap_or("").trim())
    .any(|s| s.eq_ignore_ascii_case("text/html"))
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
    .route("/json", get(json_or_html));

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

#[cfg(test)]
mod tests {
  use super::*;
  use axum::{
    http::{self, Request, StatusCode},
    Router,
  };
  use tower::util::ServiceExt;

  #[tokio::test]
  async fn test_getting_json() {
    let app = Router::new().route("/json", get(json_or_html));

    let response = app
      .oneshot(
        Request::builder()
          .method(http::Method::GET)
          .uri("/json")
          .header(http::header::ACCEPT, mime::APPLICATION_JSON.as_ref())
          .body(String::new())
          .unwrap(),
      )
      .await
      .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(response
      .headers()
      .get(http::header::CONTENT_TYPE)
      .unwrap()
      .to_str()
      .unwrap()
      .split(";")
      .any(|x| x.trim() == "application/json"));
  }

  #[tokio::test]
  async fn test_getting_html() {
    let app = Router::new().route("/json", get(json_or_html));

    let response = app
      .oneshot(
        Request::builder()
          .method(http::Method::GET)
          .uri("/json")
          .header(http::header::ACCEPT, mime::TEXT_HTML.as_ref())
          .body(String::new())
          .unwrap(),
      )
      .await
      .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(response
      .headers()
      .get(http::header::CONTENT_TYPE)
      .unwrap()
      .to_str()
      .unwrap()
      .split(";")
      .any(|x| x.trim() == "text/html"));
  }

  #[test]
  fn test_accept_header_contains_text_html() {
    let mut headers = HeaderMap::new();
    headers.insert(header::ACCEPT, "text/html".parse().unwrap());
    assert!(accept_header_contains_text_html(&headers));

    headers.insert(header::ACCEPT, "application/json, Text/Html; q=0.5".parse().unwrap());
    assert!(accept_header_contains_text_html(&headers));

    headers.clear();
    headers.insert(header::ACCEPT, "application/xml".parse().unwrap());
    assert!(!accept_header_contains_text_html(&headers));
  }
}
