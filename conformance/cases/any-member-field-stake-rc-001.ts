// rotation 325 — an any-member read of an ANY-TYPED class field
// (inferred `arr = [1, 2]`, AggregateError's injected `errors`) rode
// the compile-time candidate arm, whose inc gate skipped Type::Any
// slots. The NaN-box passed through untouched while the read's owned
// contract stood, so the consumer's release stole the field slot's
// only stake: a discarded `e.arr` freed the array out from under the
// live instance (zero incs, two decs on the census ledger). The
// typed-field inc was already there; the Any slot now takes the
// box-gated one.
class K {
  arr = [1, 2];
}
const e: any = new K();
e.arr;
console.log(e.arr.length, e.arr[1]);
const errs = [new Error("a"), new Error("b")];
const ag: any = new AggregateError(errs, "multi");
ag.errors;
console.log(ag.errors.length, ag.errors[0].message);
try {
  throw new AggregateError([1, 2], "boom");
} catch (err) {
  console.log("caught:", err.name, err.errors.length);
}
console.log("done");
