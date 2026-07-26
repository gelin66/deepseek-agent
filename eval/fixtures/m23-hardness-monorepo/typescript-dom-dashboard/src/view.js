export function renderRuns(list, empty, runs) {
  list.replaceChildren(
    ...runs.map((run) => {
      const item = document.createElement("li");
      item.dataset.runId = run.id;
      item.textContent = `${run.id}:${run.status}`;
      return item;
    }),
  );
  empty.hidden = true;
}
