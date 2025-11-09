import { mount } from "svelte";
import App from "./App.svelte";
import "./styles/globals.css";

const target = document.getElementById("app");

let app: any = null;

if (!target) {
  console.error("Could not find app element!");
  document.body.innerHTML =
    '<h1 style="color: red;">Error: Could not find app element!</h1>';
} else {
  try {
    app = mount(App, {
      target: target,
    });
  } catch (error) {
    console.error("Error mounting app:", error);
    document.body.innerHTML = `<h1 style="color: red;">Error: ${error}</h1>`;
  }
}

export default app;
