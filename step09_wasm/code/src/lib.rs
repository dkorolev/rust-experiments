use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
  fn js_callback(msg: &str);
}

#[wasm_bindgen]
pub fn greet(name: &str) -> String {
  js_callback(&format!("Callback from Rust, says {name}."));
  format!("Wasm works, says {name}.")
}
