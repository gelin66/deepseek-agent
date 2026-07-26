export type RequestBudget = {
  requests: number;
  seconds: number;
};

export function parseRequestBudget(
  value: string | undefined,
): RequestBudget | null {
  if (value === undefined) {
    return null;
  }
  const fields = Object.fromEntries(
    value.split(";").map((field) => {
      const [name, raw] = field.split("=", 2);
      return [name?.trim(), Number.parseInt(raw ?? "", 10)];
    }),
  );
  if (!(fields.requests > 0) || !(fields.seconds > 0)) {
    return null;
  }
  return {
    requests: fields.requests,
    seconds: fields.seconds,
  };
}
