export type CachePolicy = {
  cacheable: boolean;
  ttlSeconds: number | null;
};

export function parseCacheControl(value: string): CachePolicy {
  const directives = value.split(",").map((part) => part.trim());
  if (directives.includes("no-store")) {
    return { cacheable: false, ttlSeconds: null };
  }
  const maxAge = directives.find((part) => part.startsWith("max-age="));
  return {
    cacheable: true,
    ttlSeconds: maxAge ? Number(maxAge.slice("max-age=".length)) : null,
  };
}
