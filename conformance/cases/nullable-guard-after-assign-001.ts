// A null guard asks about the binding: can this thing be null here?
// A straight-line assignment narrow is a belief about one segment,
// and the ledger that holds it stores the DECLARED type precisely
// because the binding can still be assigned null afterwards. The
// guard was asking the live type instead, so once an assignment had
// narrowed the binding the cond no longer looked nullable and no
// narrow was collected at all.
//
// For `if (x !== null)` that was harmless — the then-branch runs on
// the surviving assignment narrow. For `if (x === null) {} else {}`
// it was fatal: the flush at the end of the then-branch returns the
// binding to the union, and the else-branch had nothing to restore
// it FROM. `q = {x: 1}; if (q === null) {} else { q.x }` was refused
// outright, a program with no null in it anywhere.

type O = { x: number };

// The else-branch of an `=== null` guard.
let q: O | null;
console.log("pre:", q);
q = { x: 99 };
if (q === null) {
  console.log("q: none");
} else {
  console.log("q:", q.x);
}

// The then-branch of the `!==` spelling keeps working.
let r: O | null;
console.log("pre:", r);
r = { x: 7 };
if (r !== null) {
  console.log("r:", r.x);
}

// The truthy spelling asks the same question.
let s: string | null;
console.log("pre:", s);
s = "hi";
if (!s) {
  console.log("s: none");
} else {
  console.log("s:", s.length);
}

// A scalar rides its boxed lane through both halves.
let n: number | null;
console.log("pre:", n);
n = 3;
if (n === null) {
  console.log("n: none");
} else {
  console.log("n:", n + 1);
}

// The guard's restore must not resurrect the assignment narrow it
// found on the way in: the flush retires those, and the binding can
// be assigned null again after the guard.
let t: string | null;
console.log("pre:", t);
t = "hi";
if (t !== null) {
  console.log("t:", t.length);
}
t = null;
console.log("t after:", t);

// A guard with no prior narrow behaves exactly as it always did.
let u: string | null = null;
if (u !== null) {
  console.log("u:", u.length);
} else {
  console.log("u: none");
}
u = "x";
console.log("u after:", u);

// Nested guards over the same binding.
let v: O | null;
console.log("pre:", v);
v = { x: 5 };
if (v === null) {
  console.log("v: none");
} else {
  if (v !== null) {
    console.log("v:", v.x);
  }
}
v = null;
console.log("v after:", v);
