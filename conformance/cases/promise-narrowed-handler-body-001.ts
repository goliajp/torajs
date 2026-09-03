// The signature of a narrowed promise handler was reconciled one
// rotation back; its BODY was still checked against what the
// declaration-reading pass had written. `s = "hi";
// Promise.resolve(s).then((v) => v + "!")` was refused on a program
// whose value is a string throughout, because the seed said
// `v: string | null`.
//
// That pass runs long before anything knows about narrowing, so it
// cannot answer whether the value reaching the handler is null. `any`
// is the honest seed, and it is the lane a nullable rides anyway. An
// un-narrowed null then reaches the body as a null and fails there —
// which is where bun fails too.

let s: string | null;
console.log("pre:", s);
s = "hi";
Promise.resolve(s).then((v) => {
  console.log(v + "!");
  console.log(v.length);
});

// pointer-shaped through a struct
type O = { x: number };
let q: O | null;
console.log("pre:", q);
q = { x: 1 };
Promise.resolve(q).then((v) => {
  console.log(v.x);
});

// the scalar spellings, which ride the same lane
let n: number | null;
console.log("pre:", n);
n = 3;
Promise.resolve(n).then((v) => {
  console.log(v + 1);
});

let b: boolean | null;
console.log("pre:", b);
b = true;
Promise.resolve(b).then((v) => {
  console.log(v ? "yes" : "no");
});

// a guard narrows the same way the assignment does
const g: string | null = "gg";
if (g !== null) {
  Promise.resolve(g).then((v) => {
    console.log(v.toUpperCase());
  });
}

// an un-narrowed null reaches the body as a null
const u: string | null = null;
Promise.resolve(u).then((v) => {
  console.log(v);
});

// an explicit annotation on the handler is kept, not seeded over
let d: string | null;
console.log("pre:", d);
d = "dd";
Promise.resolve(d).then((v: string) => {
  console.log(v + "!");
});

// non-nullable sources keep their seed — an f64 one especially
Promise.resolve(3.5).then((v) => {
  console.log(v + 1);
});
Promise.resolve("a")
  .then((v) => v + "b")
  .then((v) => {
    console.log(v);
  });
