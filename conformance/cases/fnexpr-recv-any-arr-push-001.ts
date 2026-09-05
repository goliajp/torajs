// A `this`-using function expression pushed into an `any[]` used to
// cost its binding the promoted receiver, so the program refused to
// compile. The array-LITERAL spellings were admitted in 589-03 and
// 590-01; this is the same value reaching the same kind of slot
// through a container that was declared empty.
//
// Shortest proof in the escaping family: an `any[]` binding is an
// `Arr<Any>` by its own declared type, so the element slot is Any
// with no inference to trust and no repr to case-split. Reading it
// back yields an AnyValue however it is spelled, and every any-lane
// call path shifts argv on FLAG_CLOSURE_RECV_FIRST.
let ctor = function (this: any) {
  this.q = 1;
  return this;
};

const pushed: any[] = [];
pushed.push(ctor);
console.log((new (pushed[0] as any)() as any).q);

// unshift stores an element the same way, and `Array<any>` is the
// same type spelled differently
const fronted: Array<any> = [];
fronted.unshift(ctor);
console.log((new (fronted[0] as any)() as any).q);

// every argument of one call is an element
const many: any[] = [];
many.push(ctor, ctor);
console.log(many.length, (new (many[1] as any)() as any).q);

// a DETACHED read is a plain call, so its receiver is undefined
// (§10.2.1.2) — the promoted body has to see that, not a shifted
// first argument
let probe = function (this: any) {
  return this === undefined;
};
const held: any[] = [];
held.push(probe);
const detached = held[0];
console.log((detached as any)());

// declared inside a function body, and constructed more than once —
// the premature-free witness the sibling fixtures carry
function inner() {
  const local: any[] = [];
  local.push(ctor);
  const a: any = new (local[0] as any)();
  const b: any = new (local[0] as any)();
  const c: any = new (local[0] as any)();
  return [a.q, b.q, c.q, typeof local[0]];
}
console.log(inner().join(","));
