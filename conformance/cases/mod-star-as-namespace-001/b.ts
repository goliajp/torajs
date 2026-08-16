// `export * as ns from "m"` exposes exactly ONE name — the namespace
// object itself — rather than m's individual exports.
export * as inner from "./a.ts";
export const OUTER = "outer";
