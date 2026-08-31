// An un-annotated top-level object literal promotes to a data global
// under a spelling built field by field, and one field the spelling
// could not name kept the WHOLE binding main-local — so a named fn
// reading any field of it failed to compile. A computed field whose
// type the spec fixes and whose representation has no width question
// can be named like a literal one.
const b1: string = "b";
const x: number = 5;

const o1 = { msg: "a" + b1, tag: "lit", n: 3 };
function readMsg(): string {
  return o1.msg + "|" + o1.tag + "|" + o1.n;
}
console.log(readMsg(), o1.msg, o1.msg.length);

const o2 = { ok: x > 1, same: x === 5, negated: !(x > 1) };
function readFlags(): string {
  return o2.ok + "|" + o2.same + "|" + o2.negated;
}
console.log(readFlags(), typeof o2.ok);

// a field written from a named fn body lands where the main side
// reads it
const o3 = { label: "v" + x };
function relabel(): void {
  o3.label = o3.label + "!";
}
relabel();
relabel();
console.log(o3.label);

// numeric computed fields stay out — their width belongs to a
// different slot key than the binding's — and the main side reads
// them as it always has
const o4 = { v: 1 + 1, w: 7 / 2 };
console.log(o4.v, o4.w, typeof o4.v);
