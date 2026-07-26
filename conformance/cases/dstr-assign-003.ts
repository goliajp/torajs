// S2.24 (RFC 20260727-dstr-assignment 刀 1) — object destructuring
// assignment: shorthand, renamed, member targets, defaults, and the
// §13.15.5 RequireObjectCoercible guard on a null source.

// shorthand fields write the same-named bindings
let x = 0;
let y = 0;
({ x, y } = { x: 1, y: 2 });
console.log(x, y); // 1 2

// renamed field → different binding
let rn = 0;
({ k: rn } = { k: 7 });
console.log(rn); // 7

// member target
let o = { a: 0 };
({ p: o.a } = { p: 5 });
console.log(o.a); // 5

// default fires on a missing field (any source), not on a present one
let src: any = { m: 3 };
let d = 0;
({ m: d = 9 } = src);
console.log(d); // 3
let src2: any = {};
let e = 0;
({ m: e = 9 } = src2);
console.log(e); // 9

// null source → TypeError before any field read
let z = 0;
try {
  ({ x: z } = null as any);
} catch (err) {
  console.log("caught"); // caught
}
console.log(z); // 0
