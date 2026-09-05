// The `any[]` push proof with one more step to reach the annotation.
// An `any[]` binding spells its element type on the binding; a keyed
// collection spells it in a GENERIC ARGUMENT, which arrives either on
// the binding (`const m: Map<string, any>`) or on the constructor
// (`const m = new Map<string, any>()`). Both are how the type is
// written, so both are read. Once the slot is known to be Any the
// rest is the push proof verbatim.
//
// Only the VALUE positions are admitted — `set`'s second argument and
// `add`'s only one. A key is a different generic argument this proof
// says nothing about, so `new Map<any, string>(); m.set(ctor, 'v')`
// still refuses, as does a value slot spelled anything narrower.
let ctor = function (this: any) {
  this.q = 1;
  return this;
};

// Constructed more than once: one construction can leave a dangling
// reference a walk still reads as plausible bytes.
const viaCtorArgs = new Map<string, any>();
viaCtorArgs.set("a", ctor);
console.log(
  (new (viaCtorArgs.get("a") as any)() as any).q,
  (new (viaCtorArgs.get("a") as any)() as any).q,
);

const viaAnnotation: Map<string, any> = new Map();
viaAnnotation.set("a", ctor);
console.log((new (viaAnnotation.get("a") as any)() as any).q);

// `Set<any>` in both spellings — the element slot is the value slot.
const setCtorArgs = new Set<any>();
setCtorArgs.add(ctor);
const setAnnotated: Set<any> = new Set();
setAnnotated.add(ctor);
console.log(setCtorArgs.size, setAnnotated.size);

// A conditional in the value position distributes, as it does in
// every other exactly-`any` slot.
const pick = true;
const joined = new Map<string, any>();
joined.set("a", pick ? ctor : ctor);
console.log((new (joined.get("a") as any)() as any).q);

// The receiver really arrives through the collection.
let seenThis = function (this: any) {
  return this === undefined;
};
const held = new Map<string, any>();
held.set("a", seenThis);
console.log((held.get("a") as any)(), (held.get("a") as any)());

// A nested generic does NOT make the OUTER value slot `any` — the
// outer map's values are maps. The inner one still admits, which is
// what lets this program compile at all.
const outer = new Map<string, Map<string, any>>();
const inner = new Map<string, any>();
inner.set("x", ctor);
outer.set("a", inner);
console.log((new (outer.get("a")!.get("x") as any)() as any).q);
