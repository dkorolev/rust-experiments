use askama::Template;

#[derive(Template)]
#[template(path = "ws.html")]
pub struct WsHtmlTemplate<'a> {
  pub ws_address: &'a str,
}
