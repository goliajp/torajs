// `typeof` is a consumer of an owned `any` member read (chunk 717) and
// never took the release over: `typeof o.f` stranded the read's +1, so
// a FRESH receiver per iteration leaked its whole payload. This fixture
// pins the behaviour; the leak itself is a churn probe (AOT RSS over
// 300k iterations: 24.3 MB before, 6.0 MB after — flat against a 5.9 MB
// baseline).

const o: any = { f: (): number => 1, s: "x" + "y", a: 1 };
console.log(typeof o.f, typeof o.s, typeof o.a);

// the operand keeps working after the typeof (the drop releases the
// read's reference, not the receiver's stake in the field).
console.log(o.s, o.a, typeof o.s);
const g: any = o.f;
console.log(typeof g, g());

// a fresh receiver per iteration is the shape that leaked — the values
// must still be right after the drop lands.
let seen = 0;
for (let i = 0; i < 3; i++) {
  const fresh: any = { f: (): number => i, s: "v" + i };
  if (typeof fresh.f === "function" && typeof fresh.s === "string") {
    seen += 1;
  }
}
console.log(seen);
