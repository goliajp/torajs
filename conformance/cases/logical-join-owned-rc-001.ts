// rotation 325 — a short-circuit `&&` / `||` whose rhs arm answers an
// owned value (an any-member read like `e.constructor`) must unify
// the join to owned on BOTH paths, the chunk-722 ternary contract.
// Off that track a DISCARDED `e && e.constructor` had no release
// site anywhere: expr_owned_shape called the whole join a borrow
// while the rhs arm's +1 rode into the slot. The strand sat on the
// TypeError class-object through the at-exit cycle drain and cut
// the error-prototype cycle — the underflow the census flagged on
// proto-own-undefined-read-001 / proto-delete-restore-001 /
// proto-patch-arr-fn-001 (their catch arms all spell
// `e && e.constructor ? e.constructor.name : "?"`).
const e: any = new TypeError("x");
const c: any = e && e.constructor;
console.log(c === TypeError);
e && e.constructor;
console.log(e && e.constructor ? e.constructor.name : "?");
const f: any = null;
console.log(f && f.constructor ? "y" : "n");
const g: any = new RangeError("r");
const h: any = g.nope || g.constructor;
console.log(h === RangeError);
console.log("done");
