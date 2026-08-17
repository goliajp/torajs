// The state cell injects ONCE (both fns close over the same mangled
// var); a hidden-mangled EXPORT stays importable by a later plain
// request (the mangle memory must not trip the requested-collision
// reject). `b` itself is deliberately NOT imported — a requested
// bare-export face rename breaking sibling references is a distinct
// pre-existing defect (L3b, rotation 427).
import d, { inc, c } from "./lib_state.ts";
import { k } from "./lib_twice.ts";
import { util } from "./lib_twice.ts";
console.log(inc(), inc(), inc());
console.log(c, d);
console.log(k, util());
