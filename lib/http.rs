use askama::Template;
use axum::response::{Html, IntoResponse, Response};
use hyper::{
  header::{self, HeaderMap},
  StatusCode,
};

#[derive(Template)]
#[template(path = "jsontemplate.html", escape = "none")]
pub struct DataHtmlTemplate<'a> {
  pub raw_json_as_string: &'a str,
}

pub async fn json_or_html(headers: HeaderMap, raw_json_as_string: &str) -> impl IntoResponse {
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

pub fn accept_header_contains_text_html(headers: &HeaderMap) -> bool {
  headers
    .get_all(header::ACCEPT)
    .iter()
    .filter_map(|s| s.to_str().ok())
    .flat_map(|s| s.split(','))
    .map(|s| s.split(';').next().unwrap_or("").trim())
    .any(|s| s.eq_ignore_ascii_case("text/html"))
}

#[cfg(test)]
mod tests {
  use super::*;
  use axum::{
    http::{self, Request, StatusCode},
    routing::get,
    Router,
  };
  use tower::util::ServiceExt;

  #[tokio::test]
  async fn test_getting_json() {
    let app =
      Router::new().route("/json", get(|headers| async move { json_or_html(headers, r#"{"test":"data"}"#).await }));

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
    let app =
      Router::new().route("/json", get(|headers| async move { json_or_html(headers, r#"{"test":"data"}"#).await }));

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
