// chunk 794 — depth-0 splitters treated the `>` of a fn-type return
// arrow (`__cls(a|b)->r`) as a generic closer, so an inline object
// annotation with a fn-typed field never split past it: the
// optional-field fill saw one bogus field (loud checker reject on
// the short literal), and the SSA `__struct` decoder swallowed the
// rest of the field list the same way. Both splitters now skip
// exactly the `->` pair. String/number optional fields (no arrow)
// exercised alongside as the already-green control.
const c: { cb?: () => number; n: number } = { n: 1 };
console.log(c.cb?.length ?? -2);
console.log(c.n);

function take2(o: { cb?: () => number; n: number }): number {
  return o.cb ? o.cb() : -9;
}
console.log(take2({ n: 2 }));
console.log(take2({ cb: () => 21, n: 3 }));

// fn field with params, not in first position
const e: { g: string; f?: (n: number) => number } = { g: "x" };
console.log(e.f?.length ?? -1);
console.log(e.g);

// two optional fields around a required one
const m: { a?: string; k: number; b?: boolean } = { k: 7 };
console.log(m.a ?? "-");
console.log(m.k);
console.log(m.b ?? false);

// control: optional string field (was already green)
const d: { tag?: string; n: number } = { n: 4 };
console.log(d.tag ?? "-");
