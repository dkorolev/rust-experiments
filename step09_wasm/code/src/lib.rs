use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
  fn js_log(msg: &str);
  fn js_return_another_name() -> String;
}

#[wasm_bindgen]
pub fn greet(name: &str) -> String {
  js_log(&format!("Callback from Rust, says {name}."));
  let another_name = js_return_another_name();
  js_log(&format!("Another callback from Rust, says {another_name}."));
  format!("Wasm works, says {name}.")
}
