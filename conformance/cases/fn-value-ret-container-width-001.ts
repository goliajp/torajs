// rotation 553 — a call through an fn-valued binding now answers a
// container key (the value's `__ret` projection, the same spelling the
// width face and the `__p{i}` arg edges already used), so the binding
// holding its result joins the callee's ret class. Before, the result
// had NO key: `a(boom())` legitimately widened `a`'s return elements
// to f64 (an `any` argument reaches the element slots), the callee's
// signature widened with it, and `r` stayed narrow — a reassignment
// died on the slot-fit mismatch and the init form silently bit-punned
// f64 slots as integers (printed 4607182418800017408,…).
const a = (n: number): number[] => [n, n + 1];
const boom = (): any => {
  throw new Error("x");
};

// Reassignment form (the panic face).
let r: number[] = [];
r = a(1);
r = a(2).concat(a(3), a(4));
console.log("reassign", r.join(","), r.length);

// Init form (the silent bit-pun face).
const q: number[] = a(5);
console.log("init", q.join(","));

// Non-ident callee (`fs[0]()`) — the projection hangs off the Elem key.
const fs = [a];
let e: number[] = [];
e = fs[0](9);
console.log("elem", e.join(","));

// The widening trigger: an `any` actual reaching the callee.
let caught = 0;
for (let i = 0; i < 3; i++) {
  try {
    a(boom());
  } catch (err) {
    caught++;
  }
}
console.log("throw", caught);
