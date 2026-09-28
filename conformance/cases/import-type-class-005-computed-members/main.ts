// a type-only class import whose declaration carries evaluation of
// its own — static block, static field, runtime computed member keys
// — lands for its shape alone: none of it evaluates, m never runs
import type { S } from "./m";
function f(s: S): string { return s.tag() + s.lit(); }
let held: S | undefined;
console.log(typeof f, f.length, held === undefined);
