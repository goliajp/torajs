// Rotation 204 — degraded top-level object bindings are named-fn
// visible (registered as Any globals, exactly like a `: any`
// annotation), and the dynobj define write-back reaches the global
// slot (the old locals-only write-back silently stranded a
// relocating define on the stale cell).

// face 1 — degraded binding (free-receiver define in a named fn
// body) read from another named fn: was "unknown identifier".
let g = { b: 1 };
function readB() {
  return g.b;
}
function defineC() {
  Object.defineProperty(g, "c", { value: 9, enumerable: true });
}
defineC();
console.log(readB());
console.log(g.c);
console.log(Object.keys(g).length);

// face 2 — annotated `any` global + top-level define: the write-back
// used to miss the global slot and `h.c` read undefined.
let h: any = { b: 1 };
Object.defineProperty(h, "c", { value: 2 });
function readHC() {
  return h.c;
}
console.log(readHC());
console.log(h.b);

// face 3 — degraded binding reassigned after a define (the mutable
// Any-global assign lane owns drop-old/box-new).
let m = { b: 1 };
Object.defineProperty(m, "x", { value: 5 });
m = { b: 2 };
function readM() {
  return m.b;
}
console.log(readM());

// face 4 — closure capture and named fn share the promoted global
// single home.
let s = { b: 1 };
function readS() {
  return s.b;
}
const cbS = () => s.b + 10;
Object.defineProperty(s, "c", { value: 2 });
console.log(readS());
console.log(cbS());
console.log(s.c);
