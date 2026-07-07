// Chunk 653 — a Substr element in an array literal materializes to
// an owned Str before the slot store. Pre-fix the view POINTER went
// into the Str-typed slot (Substr block layout != Str layout past
// the header), so join/print/sort read garbage bytes; a Substr
// FIRST element anchored an Arr<Substr> and crashed outright.

// 1) Str anchor + index-view element (e4 shape).
const s = "ab";
const xs = ["z", s[1], "a"];
console.log(xs.join(","));
xs.sort();
console.log(xs.join(","));

// 2) Substr anchor (first element is a view) — no Arr<Substr>.
const t = "hello";
const ys = [t[1], "z"];
console.log(ys.join(","));

// 3) All-view literal.
const zs = [t[0], t[4]];
console.log(zs.join(","));

// 4) OOB index element — undefined sentinel propagates through
// substr_to_owned (join renders empty per §23.1.3.18).
const ws = ["a", t[99], "c"];
console.log(ws.join(","));
console.log(ws[1] === undefined);

// 5) Ident-bound view element — binding stays live after the store.
const sub = t.slice(1, 4);
const vs = [sub, "q"];
console.log(vs.join(","));
console.log(sub);

// 6) Method-call view element (fresh owned view producer).
const ms = [t.slice(0, 2), "x"];
console.log(ms.join(","));
