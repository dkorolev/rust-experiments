use axum::body::Body;
use axum::{routing::get, serve, Router};
use hyper::header::CONTENT_TYPE;
use hyper::StatusCode;

use tower::ServiceBuilder;

use hyper::header::ACCEPT;
use hyper::Request;
use std::net::SocketAddr;
use tokio::signal::unix::{signal, SignalKind};
use tokio::{net::TcpListener, sync::mpsc};

use axum::middleware::{from_fn, Next};
use axum::response::{IntoResponse, Response};

use askama::Template;

#[derive(Clone)]
struct BrowserFriendlyJson {
  data: String,
}

#[derive(Template)]
#[template(path = "jsontemplate.html", escape = "none")]
struct DataHtmlTemplate<'a> {
  raw_json_as_string: &'a str,
}

impl IntoResponse for BrowserFriendlyJson {
  fn into_response(self) -> Response {
    let mut response = StatusCode::NOT_IMPLEMENTED.into_response();
    response.extensions_mut().insert(self);
    response
  }
}

const SAMPLE_JSON: &str = include_str!("sample.json");

fn create_response<S: Into<String>>(content_type: &str, body: S) -> Response<Body> {
  Response::builder().status(StatusCode::OK).header(CONTENT_TYPE, content_type).body(Body::from(body.into())).unwrap()
}

async fn browser_json_renderer(request: Request<Body>, next: Next) -> Response {
  // TODO(dkorolev): Can this be more Rusty?
  let mut accept_html = false;
  request.headers().get(&ACCEPT).map(|value| {
    let s = std::str::from_utf8(value.as_ref()).unwrap();
    s.split(',').for_each(|value| {
      if value == "text/html" || value == "html" {
        accept_html = true;
      }
    })
  });

  // NOTE(dkorolev): I could not put the above logic to inside after `if let`, although, clearly it should be there.
  let mut response = next.run(request).await;
  if let Some(my_data) = response.extensions_mut().remove::<BrowserFriendlyJson>() {
    if accept_html {
      let template = DataHtmlTemplate { raw_json_as_string: &my_data.data };
      if let Ok(html) = template.render() {
        return create_response("text/html", html);
      }
    }
    return create_response("application/json", my_data.data);
  }

  response
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
    .route("/json", get(|| async { BrowserFriendlyJson { data: SAMPLE_JSON.to_string() } }))
    .layer(ServiceBuilder::new().layer(from_fn({
      // TODO(dkorolev): Can I just move the `html_template` into `browser_json_renderer`?
      |req, next| browser_json_renderer(req, next)
    })));

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
