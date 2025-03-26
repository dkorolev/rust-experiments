import init, { greet } from "./pkg/wasm.js";

window.js_callback = (msg) => {
  console.log("Called the callback from Rust:", msg);
};

init().then(() => {
  document.getElementById("out").innerText = greet("Dima");
});
