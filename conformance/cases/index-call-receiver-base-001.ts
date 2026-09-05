// §13.3.6.2 EvaluateCall takes `this` from the callee REFERENCE's
// base. A type assertion does not consume a Reference, so
// `(a[0] as any)()` calls with `this === a` exactly as `a[0]()` does
// — and the same holds for the member spelling. tr dropped the base
// whenever the shape test failed to see through the `as` shell, and
// whenever an `any[]` element read (whose callee already dispatches
// dynamically) fell through to the receiverless bare any-call layer.
//
// A detached read is the one spelling that DOES drop the base
// (§10.2.1.2), and it is asserted here so the fix cannot overshoot
// into handing a receiver to a value that no longer has one.
let probe = function () {
  return typeof (this as any);
};

const anyRecv: any = [probe];
console.log(anyRecv[0](), (anyRecv[0] as any)(), (anyRecv[0])());

const anyElems: any[] = [];
anyElems.push(probe);
console.log(anyElems[0](), (anyElems[0] as any)());

const inferred = [probe];
console.log((inferred[0] as any)());

const bag: any = { f: probe };
console.log(bag.f(), (bag.f as any)(), bag["f"](), (bag["f"] as any)());

type Holder = { f: () => string };
const held: Holder = { f: probe };
console.log(held.f(), (held.f as any)());

// The base is the array itself, not merely "some object".
let who = function () {
  return (this as any) === anyRecv;
};
const ident: any = anyRecv;
ident[1] = who;
console.log(ident[1](), (ident[1] as any)());

// Arguments still land after the receiver, not shifted onto it.
let pair = function (x: any, y: any) {
  return typeof (this as any) + ":" + x + y;
};
const withArgs: any[] = [pair];
console.log(withArgs[0](1, 2), (withArgs[0] as any)(3, 4));

// Detached: the Reference is gone, so strict-mode `this` is undefined.
const detached = anyRecv[0];
console.log(detached());
