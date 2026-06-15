// `(x: any) || "default"` / `(x: any) && next` previously hit a
// check.rs strict-equality wall: "&& / || require matching
// operand types, got Any and String". The same family as
// W-D narrow trunk — typed Any vs sibling primitive needs a
// widen-to-Any escape. ssa-lower widens the slot to Any and
// NaN-boxes whichever side isn't already Any, mirroring the
// box_to_any path used by Array<Any>.reduce init.

const a: any = 0;
console.log(a || "fb");      // "fb" (0 falsy)
const b: any = "ok";
console.log(b && "next");    // "next" (truthy short-circuit)
const c: any = null;
console.log(c ?? "fb2");     // "fb2" (nullish; orthogonal arm, kept as regression guard)

// Both branches reachable
const d: any = 1;
console.log(d || "fb");      // 1 (truthy)
const e: any = "x";
console.log(e || "fb");      // "x" (truthy)
const f: any = "";
console.log(f || "fb");      // "fb" (empty string falsy)
const g: any = false;
console.log(g && 99);        // false (falsy short-circuit)
const h: any = true;
console.log(h && "yes");     // "yes"

// Left typed, right typed any
const j = 0 || (a as any);
console.log(j);              // 0 (left is 0 falsy → right; a is 0 too)
const k = "x" || (a as any);
console.log(k);              // "x"

// Right Any literal pattern
const m: any = 5;
const n: any = 10;
console.log(m || n);         // 5
console.log(m && n);         // 10

// Mixed with bool
const p: any = true;
console.log(p && false);     // false
