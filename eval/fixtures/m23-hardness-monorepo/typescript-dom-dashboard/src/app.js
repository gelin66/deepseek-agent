import { visibleRuns } from "./state.js";
import { renderRuns } from "./view.js";

const button = document.querySelector("#toggle");
const list = document.querySelector("#run-list");
const empty = document.querySelector("#empty");
let hideCompleted = false;

function render() {
  renderRuns(list, empty, visibleRuns(hideCompleted));
  button.setAttribute("aria-expanded", String(hideCompleted));
  button.textContent = hideCompleted ? "Show completed" : "Hide completed";
}

button.addEventListener("click", () => {
  hideCompleted = !hideCompleted;
  render();
});

render();
