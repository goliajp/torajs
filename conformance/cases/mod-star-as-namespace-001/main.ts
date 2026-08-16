// §16.2.3 `export * as ns from "m"`. Two requests reach the same
// clause and each needs a different local spelling:
//   - a named import wants `inner` bound at top level
//   - a namespace import wants `inner` as a FIELD of the outer
//     namespace, whose value is the inner namespace object
// The second is why r421 emits the synthetic `let` bindings in reverse
// discovery order — the outer object names the inner one, and the
// inner was discovered while walking the outer.
import { inner, OUTER } from "./b.ts";
import * as outer from "./c.ts";
console.log(OUTER);
console.log(inner.WA);
console.log(inner.fwa());
console.log(outer.OUTER2);
console.log(outer.inner.WA);
