import { mount } from "svelte";
import App from "./App.svelte";
import "./styles/globals.css";

if (import.meta.env.MODE === "development") {
  console.log("Main.ts loading...");
}
console.log("Main.ts loading...");

const target = document.getElementById("app");
if (import.meta.env.MODE === "development") {
  console.log("Target element:", target);
}
console.log("Target element:", target);

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
    if (import.meta.env.MODE === "development") {
      console.log("App mounted successfully");
    }
    console.log("App mounted successfully");
  } catch (error) {
    console.error("Error mounting app:", error);
    document.body.innerHTML = `<h1 style="color: red;">Error: ${error}</h1>`;
  }
}

export default app;
