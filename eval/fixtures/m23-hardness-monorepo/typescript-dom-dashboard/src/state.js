export const runs = Object.freeze([
  { id: "run-1", status: "active" },
  { id: "run-2", status: "completed" },
]);

export function visibleRuns(hideCompleted) {
  return hideCompleted ? runs.filter((run) => run.status !== "completed") : [...runs];
}
