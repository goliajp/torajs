// An array element annotated `T | null` rides the any lane for every
// T. For a pointer-shaped T the slot itself was always fine — its
// null is the in-band 0 a pointer has spare — but the typed element
// lane could not say so: the literal died at runtime with "array
// element does not match the annotated element type", and the
// empty-then-push spelling printed a diagnostic tag. This is the
// annotated half of the rule the inferred array literal already had.

type O = { x: number };

// literal mixing null with a live sibling
const a: (O | null)[] = [null, { x: 1 }];
console.log(a);
console.log(a.length);
console.log(a[0]);

// the element is readable once the guard has spoken
const first = a[1];
if (first !== null) {
  console.log(first.x);
}

// empty annotation, filled by push
const b: (O | null)[] = [];
b.push(null);
b.push({ x: 2 });
console.log(b);

// string elements — the same lane, a different pointer shape
const s: (string | null)[] = [null, "s"];
console.log(s);
console.log(s[1]);

// an array-typed element, and a nested one
const n: (number[] | null)[] = [null, [1, 2]];
console.log(n);

// walking it
for (const e of a) {
  console.log(e);
}

// the scalar spellings this rule already covered stay put
const q: (number | null)[] = [1, null, 3];
console.log(q);
console.log(q.filter((v) => v !== null).length);

const t: (boolean | null)[] = [true, null];
console.log(t);

// plain (non-nullable) element annotations keep the typed lane
const p: O[] = [{ x: 3 }];
console.log(p[0].x);
const r: string[] = ["k"];
console.log(r.join("-"), r.length);

// a class instance is pointer-shaped too
class C {
  v = 7;
}
const c: (C | null)[] = [null, new C()];
console.log(c);
const c1 = c[1];
if (c1 !== null) {
  console.log(c1.v);
}

// assignment through the slot, both ways
let m: (O | null)[] = [{ x: 4 }];
m = [null];
console.log(m);
m[0] = { x: 5 };
console.log(m);
m[0] = null;
console.log(m);
