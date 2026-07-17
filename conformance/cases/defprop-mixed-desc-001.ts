// §6.2.6.5 steps 9/10 — a descriptor mixing an accessor face with
// `value` / `writable` throws TypeError. The literal fast path
// stored the AccessorPair and silently dropped the value field;
// it now declines so the runtime ToPropertyDescriptor rejects.

const setFun: any = function (v: any) {};
const getFun: any = function () { return 1; };

function expectThrow(tag: string, run: () => void): void {
  try {
    run();
    console.log(tag, "no throw");
  } catch (e) {
    console.log(tag, e instanceof TypeError);
  }
}

expectThrow("value+set:", () => {
  Object.defineProperties({} as any, { prop: { value: 12, set: setFun } });
});
expectThrow("value+get:", () => {
  Object.defineProperty({} as any, "p", { value: 1, get: getFun });
});
expectThrow("writable+get:", () => {
  Object.defineProperty({} as any, "p", { writable: true, get: getFun });
});

// pure accessor and pure data descriptors keep working (inline
// method faces; an any-typed NAMED-fn-expr face SIGSEGVs on its
// own — recorded separately, not this fixture's surface)
const o: any = {};
Object.defineProperty(o, "a", { get() { return 1; }, enumerable: true });
console.log(o.a); // 1
Object.defineProperty(o, "d", { value: 5, writable: true });
console.log(o.d); // 5
console.log("done");
