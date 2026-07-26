// Experiment retained as a localization decoy. Production does not import it.
export function matchRoute(pattern: RegExp, pathname: string): boolean {
  return pattern.test(pathname);
}
