// Second hub carrying the same clause — the outer-namespace request
// goes through this one because `import { x } from "m"` and
// `import * as ns from "m"` against ONE module is a separate, known
// gap (the second request re-injects the first's decls).
export * as inner from "./a.ts";
export const OUTER2 = "outer2";
