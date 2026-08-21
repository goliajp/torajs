// RFC 20260821 A2 — an Array<Str>/Array<Substr> is released by one
// kernel call instead of an SSA-emitted per-slot walk. The kernel
// skips a slot only when releasing it provably does nothing (static
// literal, or an inline substring whose parent is one), so this pins
// the shapes where it must NOT skip, plus the two layout details the
// walk has to respect: the deque head and a sparse tail.

// deque head != 0 after shifting
const d: string[] = ["a", "b", "c", "d"];
d.shift();
d.shift();
console.log(d.join(","), d.length);

// owned (non-literal) parent — the substrings genuinely owe it a
// decrement, and the parent must not outlive them or die early
function mk(n: number): string[] {
  const s: string = "x" + n.toString() + " y z";
  return s.split(" ");
}
let tot: number = 0;
for (let i: number = 0; i < 2000; i = i + 1) {
  const p: string[] = mk(i);
  tot = tot + p.length + p[0].length;
}
console.log(tot);

// shared array (refcount > 1): the last owner walks, not the first.
// `let` rather than `const` on purpose: a top-level annotated `const`
// is promoted to a data global whose slot takes the ANNOTATION's
// layout, so `const a: string[] = s.split(" ")` loses the fact that
// the elements are substring views and `join` then reads them by the
// wrong layout. That is a separate, pre-existing defect (it predates
// this change and every commit in this rotation) and is fixed on its
// own; this fixture is about the element release.
let a: string[] = "p q r".split(" ");
let b: string[] = a;
console.log(a.join("-"), b.join("-"), b[2]);

// sparse tail — slots past the live extent have no storage
const sp: string[] = [];
sp[0] = "z";
sp[5] = "w";
console.log(sp.length, sp[0], sp[5]);

// owned strings rather than substring views
const o: string[] = ["m" + "n", "o" + "p"];
console.log(o.join("/"));

// literal-parent substrings, the shape the fast skip is for
let lit: string[] = "3 4 + 2 * 5 +".split(" ");
console.log(lit.length, lit[0], lit[6]);

// empty and single-element results
console.log("".split(",").length, "solo".split(",")[0]);
