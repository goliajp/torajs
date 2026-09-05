// The base case the receiver-safe family never had. Writing a
// `this`-using function expression into an `any`-annotated binding —
// the shortest spelling of the any-lane escape — was an unclaimed
// position, so `const v: any = k` refused to compile while
// `const v: any = [k]` and `const v: any = { f: k }` both compiled.
//
// The proof needs no census of the program: the slot's type is fixed
// by the annotation on this declaration, so every read hands back an
// AnyValue however it is spelled, and every any-lane call path shifts
// argv on FLAG_CLOSURE_RECV_FIRST. Unlike the unannotated alias —
// whose slot holds the raw closure repr, so a direct call through it
// would eat an argument — an `: any` slot has no typed call lane to
// fall into.
let ctor = function (this: any) {
  this.q = 1;
  return this;
};

// Constructed more than once: one construction can leave a dangling
// reference that a walk still reads as plausible bytes.
const asConst: any = ctor;
console.log((new (asConst as any)() as any).q, (new (asConst as any)() as any).q);

let asLet: any = ctor;
var asVar: any = ctor;
console.log((new (asLet as any)() as any).q, (new (asVar as any)() as any).q);

// A cast on the initializer changes the static type of the read, not
// where the value is stored.
const withCast: any = ctor as any;
console.log((new (withCast as any)() as any).q);

// Inside a block: the census walks the shared nested-list spine, so
// this declaration is seen exactly as the top-level ones are.
{
  const inBlock: any = ctor;
  console.log((new (inBlock as any)() as any).q);
}

// The receiver arrives, not merely "some object".
let seenThis = function (this: any) {
  return this === undefined;
};
const detachedThis: any = seenThis;
console.log(detachedThis(), detachedThis());

// A variadic body rides the same slot. This shape carries no variadic
// exclusion — that exclusion exists for array elements, whose type is
// not spelled anywhere the census can see, and here it is spelled.
let counted = function (this: any, ...xs: any[]) {
  return xs.length;
};
const variadic: any = counted;
console.log(variadic(1, 2, 3), variadic(4), variadic());
