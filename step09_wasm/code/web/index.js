import init, { greet } from "./pkg/wasm.js";

window.js_log = (msg) => {
  console.log("Called the callback from Rust:", msg);
};

window.js_return_another_name = () => {
  return "Max";
};

init().then(() => {
  document.getElementById("out").innerText = greet("Dima");
});
