// §10.5.14 step 7 — a Proxy has [[Call]] exactly when its target
// does, fixed at creation. The question is §7.2.3 IsCallable, and the
// copy that answered it here listed the closure shapes only, so a
// Proxy over a class recorded itself non-callable: `typeof` said
// "object" about a value the spec calls a function.
function attempt(f: any): string {
  try {
    return "ran:" + String(f());
  } catch (e: any) {
    return e && e.constructor ? e.constructor.name : "?";
  }
}
class C {
  m(): number {
    return 1;
  }
}
const pc: any = new Proxy(C as any, {});
console.log("typeof class-target ", typeof pc);
console.log("call  ", attempt(() => pc()));
console.log("new   ", attempt(() => new pc().m()));

const pf: any = new Proxy(function () {
  return 7;
} as any, {});
console.log("typeof fn-target    ", typeof pf, pf());

const po: any = new Proxy({} as any, {});
console.log("typeof obj-target   ", typeof po);

const E: any = Error;
const pe: any = new Proxy(E, {});
console.log("typeof Error-target ", typeof pe);

// Not checked here: `defineProperty(o, k, {get: SomeClass})`. That
// goes through torajs-dynobj's own accessor-face predicate, which is
// another copy of this question with the same missing row — its own
// knife, because the AccessorPair slot wants a closure cell.
