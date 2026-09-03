// A pointer-shaped `T | null` was left on the typed array lane on
// the grounds that its in-band null sentinel is a real pointer
// value. It is — the slot can hold it. What the typed lane cannot do
// is tell anyone: `let q: O | null = null; console.log([q])` answered
// `[ [unknown-any-tag] ]` where bun says `[ null ]`.
//
// The any lane has the encoding for exactly this — a zero heap
// payload boxes as ANY_NULL — which is why the slot READER already
// goes through it. The literal writer now agrees with its reader.

type O = { x: number };

let q: O | null = null;
console.log([q]);

// A live cell in the same slot still answers as itself.
let r: O | null = { x: 1 };
console.log([r]);
console.log([r][0]);

// Class instances share the slot.
class C {
  a = 1;
}
let c: C | null = null;
console.log([c]);
c = new C();
console.log([c][0].a);

// Mixed with siblings, and nested.
let m: O | null = null;
console.log([m, { x: 2 }].length);
console.log([[m], [{ x: 3 }]].length);

// Arrays and closures are the same shape.
let arr: number[] | null = null;
console.log([arr]);

let fn: ((n: number) => number) | null = null;
console.log([fn]);

// The pointer-shaped scalar sibling (string) was already right and
// stays right.
let s: string | null = null;
console.log([s, "x"]);

// A `null` literal alongside a live cell keeps working.
class N {
  constructor(
    public v: number,
    public next: N | null = null,
  ) {}
}
const n = new N(1);
console.log([n, null].length, [n, null][0].v, [n, null][1]);
