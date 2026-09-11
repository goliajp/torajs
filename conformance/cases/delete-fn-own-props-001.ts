// 633-02 — `delete` on a function. §20.2.4 makes a function's own
// `length` and `name` non-writable but CONFIGURABLE, so both can be
// removed and the reads that follow continue to %Function.prototype%.
//
// Three things had to agree for this to work. The checker refused a
// Function receiver outright ("typed layouts have no removable
// properties" — true of a Struct, not of a closure cell with a props
// bag). A bare named fn is a raw address with no cell, so the delete
// had nowhere to land: the forwarder census now treats a delete base
// as a value site and wraps EVERY read of that name, `name` and
// `length` included, since those two fold statically off a raw base
// and would keep answering the declaration after the removal. And the
// typed `.name` read ignored the tombstone, so one cell told two
// stories depending on which spelling reached it.
function g(a: number, b: number): number {
  return a + b;
}
const h = function (p: number): number {
  return p;
};
const arrow = (z: number): number => z;

console.log("before", g.length, g.name, h.length, arrow.length);
console.log("del-len", delete (g as any).length);
console.log("after-len", g.length, (g as any).length);
console.log("del-name", delete (g as any).name);
console.log("after-name", JSON.stringify(g.name), JSON.stringify((g as any).name));
// Still callable, and still one object.
console.log("call", g(2, 3), (g as any) === (g as any));

// A fn-expression binding and an arrow are Closure-repr already —
// they were never the broken half, and they stay right.
console.log("h", delete (h as any).length, h.length);
console.log("arrow", delete (arrow as any).length, arrow.length);

// A declared default keeps §20.2.4.1 arity until something removes it.
function withDflt(a: number, b: number = 39): number {
  return a + b;
}
console.log("dflt", withDflt.length, delete (withDflt as any).length, withDflt.length);

// Removing a property that was never there answers true and changes
// nothing.
function untouched(q: number): number {
  return q;
}
console.log("missing", delete (untouched as any).nope, untouched.length, untouched.name);
