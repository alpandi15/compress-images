import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";
import { uiLog } from "./lib/api";

window.addEventListener("error", (e) => uiLog("error", `${e.message} @ ${e.filename}:${e.lineno}`));
window.addEventListener("unhandledrejection", (e) => uiLog("error", `unhandled rejection: ${String(e.reason)}`));

const target = document.getElementById("app");
if (!target) throw new Error("#app element missing");

export default mount(App, { target });
