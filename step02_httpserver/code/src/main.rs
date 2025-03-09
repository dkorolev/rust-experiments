use axum::body::Body;
use axum::{routing::get, serve, Router};
use hyper::header::CONTENT_TYPE;
use hyper::StatusCode;
use std::sync::Arc;

use tower::ServiceBuilder;

use hyper::header::ACCEPT;
use hyper::Request;
use std::net::SocketAddr;
use tokio::signal::unix::{signal, SignalKind};
use tokio::{net::TcpListener, sync::mpsc};

use axum::middleware::{from_fn, Next};
use axum::response::{IntoResponse, Response};

#[derive(Clone)]
struct BrowserFriendlyJson {
  data: String,
}

struct JsonHtmlTemplate<'a> {
  pre: &'a str,
  post: &'a str,
}

impl IntoResponse for BrowserFriendlyJson {
  fn into_response(self) -> Response {
    let mut response = StatusCode::NOT_IMPLEMENTED.into_response();
    response.extensions_mut().insert(self);
    response
  }
}

const SAMPLE_JSON: &str = include_str!("sample.json");

const fn find_split_position(bytes: &[u8]) -> usize {
  let mut i = 0;
  while i < bytes.len() && (bytes[i] != b'{' || bytes[i + 1] != b'}') {
    i += 1;
  }
  i
  // TODO(dkorolev): Panic if did not find `{}` or if found more than one `{}`.
  // NOTE(dkorolev): Why not create the split `str` slice at compile time, huh?
}

static JSON_TEMPLATE_HTML: &[u8] = include_bytes!("jsontemplate.html");
static JSON_TEMPLATE_HTML_SPLIT_IDX: usize = find_split_position(&JSON_TEMPLATE_HTML);

fn create_response<S: Into<String>>(content_type: &str, body: S) -> Response<Body> {
  Response::builder().status(StatusCode::OK).header(CONTENT_TYPE, content_type).body(Body::from(body.into())).unwrap()
}

async fn browser_json_renderer(request: Request<Body>, next: Next, tmpl: Arc<JsonHtmlTemplate<'_>>) -> Response {
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
      return create_response("text/html", format!("{}{}{}", tmpl.pre, my_data.data, tmpl.post));
    } else {
      return create_response("application/json", my_data.data);
    }
  }

  response
}

#[tokio::main]
async fn main() {
  // NOTE(dkorolev): Can this be done at compile time?
  let html_template = Arc::new(JsonHtmlTemplate {
    pre: std::str::from_utf8(&JSON_TEMPLATE_HTML[0..JSON_TEMPLATE_HTML_SPLIT_IDX]).expect("NON-UTF8 TEMPLATE"),
    post: std::str::from_utf8(&JSON_TEMPLATE_HTML[(JSON_TEMPLATE_HTML_SPLIT_IDX + 2)..]).expect("NON-UTF8 TEMPLATE"),
  });

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
      let html_template = Arc::clone(&html_template);
      move |req, next| browser_json_renderer(req, next, Arc::clone(&html_template))
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
