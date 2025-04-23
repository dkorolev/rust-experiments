use askama::Template;

#[derive(Template)]
#[template(path = "ws.html", escape = "none")]
pub struct WsHtmlTemplate<'a> {
  pub ws_address: &'a str,
}
