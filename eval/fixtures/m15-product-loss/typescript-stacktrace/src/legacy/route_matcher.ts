// Historical matcher retained as a localization decoy. Production does not import it.
export function matchRoute(pattern: string, pathname: string): boolean {
  return pattern === pathname;
}
