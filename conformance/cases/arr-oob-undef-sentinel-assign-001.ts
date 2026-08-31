// `let u = xs[9]` records the binding as possibly holding the
// undefined-NaN sentinel; `u = xs[9]` recorded nothing, because there
// is no let-decl to record at, so a later read answered NaN. It
// cannot be recorded when the assignment lowers either: inside a loop
// a read lowers before the assignment that taints it, and would be
// answered with the previous iteration's ignorance. So the names are
// collected before the body lowers, alongside the field names, and
// the two run to a fixpoint because they feed each other.
let xs: number[] = [1, 2, 3];

let a: number = 0;
a = xs[9];
console.log(a);
console.log(typeof a);

// A chain: each link needs the previous one already collected.
let p: number = 0;
let q: number = 0;
p = xs[9];
q = p;
console.log(q);
console.log(q === undefined);

// The read that lowers first and is tainted later.
let k: number = 0;
for (let i: number = 0; i < 2; i++) {
  console.log(k);
  k = xs[9];
}

// In-range assignments stay numbers.
let m: number = 0;
m = xs[1];
console.log(m);
console.log(typeof m);
console.log(m + 1);
