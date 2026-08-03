// r293 — fn-value store-sites the collector missed: a dynobj
// member-assign RHS (`box.prop = fn`, S13_A10), the for-in head
// hoist (`for (k in fn)` — §14.7.5 enumerates a plain fn's expando
// props: none), and any store-site INSIDE a for-of body (the walk
// had no ForOf arm at all).
function zig(): string {
  return "ziggy";
}

// dynobj member-assign — var receiver is an any destination
var box: any = {};
box.ziggy = zig;
console.log(typeof box.ziggy, box.ziggy());

// for-in over a plain fn: zero keys
function foo(): void {}
let hits = 0;
for (const k in foo) {
  hits++;
}
console.log("forin-keys", hits);

// store-site inside a for-of body
const box2: any = {};
for (const x of [1, 2]) {
  box2.cb = zig;
}
console.log(typeof box2.cb, box2.cb());
