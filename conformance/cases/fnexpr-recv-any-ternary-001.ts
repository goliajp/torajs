// A conditional in a slot position stores whichever arm it evaluates
// into that same slot, so an arm's proof is the slot's proof — the
// join adds no path the single-Ident spelling did not already have.
// The family's slot shapes each asked "peel `as`, then is it an
// Ident?", which reads `c ? k : k` as "not an Ident" and refused to
// compile it, in every one of the exactly-`any` positions at once.
//
// Distributing is deliberately limited to slots whose type is spelled
// exactly `any` (or `any[]`): that spelling is what makes each of
// those proofs unconditional. A slot with a narrower annotation still
// rejects, which is why nothing here writes one.
let ctor = function (this: any) {
  this.q = 1;
  return this;
};
const pick = true;

// Constructed more than once — one construction can leave a dangling
// reference a walk still reads as plausible bytes.
const inInit: any = pick ? ctor : ctor;
console.log((new (inInit as any)() as any).q, (new (inInit as any)() as any).q);

// Nested conditionals recurse for the same reason.
const nested: any = pick ? (pick ? ctor : ctor) : ctor;
console.log((new (nested as any)() as any).q);

// A cast inside an arm peels, exactly as it does outside one.
const withCast: any = pick ? (ctor as any) : ctor;
console.log((new (withCast as any)() as any).q);

// The array-literal element of an exactly-`any` binding.
const inElem: any = [pick ? ctor : ctor];
console.log((new (inElem[0] as any)() as any).q);

// The element pushed into an `any[]`.
const pushed: any[] = [];
pushed.push(pick ? ctor : ctor);
console.log((new (pushed[0] as any)() as any).q);

// An argument in an exactly-`any` parameter slot, and the default
// value of one (which the pipeline has already moved into the body by
// the time this census runs).
function viaArg(f: any) {
  return (new (f as any)() as any).q;
}
function viaDefault(f: any = pick ? ctor : ctor) {
  return (new (f as any)() as any).q;
}
console.log(viaArg(pick ? ctor : ctor), viaDefault(), viaDefault());

// The receiver really arrives through the join.
let seenThis = function (this: any) {
  return this === undefined;
};
const joined: any = pick ? seenThis : seenThis;
console.log(joined(), joined());
