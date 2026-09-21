// §25.5.2.3 step 2 takes the `toJSON` hook when IsCallable(toJSON).
// The any lane learned that in rotation 683; this is the other
// layer — the struct lane, which decides in the COMPILER from the
// layout, and the runtime walk, which never looked at a nominal
// cell at all.
//
// Two wrong documents came out of that, both from programs that
// compiled and ran, and no gate could see either:
//   { toJSON: proxyFn }          serialized the hook as data
//   class C { toJSON() {…} }     serialized the instance's FIELDS
// The second needs no proxy anywhere — it is plain JavaScript, and
// the most ordinary use of the hook there is.
function attempt(f: () => any): string {
  try {
    return "ran:" + String(f());
  } catch (e: any) {
    return "threw:" + (e && e.constructor ? e.constructor.name : "?");
  }
}
const pf: any = new Proxy(() => 7, {});

// A layout slot that types `any` cannot be called from a lane that
// needs a static signature, so the value takes the runtime walk.
console.log("any slot     ", attempt(() => JSON.stringify({ toJSON: pf, a: 1 })));
console.log("nested       ", attempt(() => JSON.stringify({ inner: { toJSON: pf } })));
console.log("in array     ", attempt(() => JSON.stringify([{ toJSON: pf }])));
console.log("with gap     ", attempt(() => JSON.stringify({ i: { toJSON: pf } }, null, 1)));

// A class object is callable, and §10.2.1 step 2 refuses the call —
// a TypeError, where the hook used to be serialized as `{}`.
console.log("class as hook", attempt(() => JSON.stringify({ toJSON: (class K {}) as any })));

// The hook in a class's METHOD table — invisible to the layout.
class C {
  a = 1;
  toJSON() {
    return 9;
  }
}
class D extends C {}
class Plain {
  a = 2;
}
console.log("class method ", attempt(() => JSON.stringify(new C())));
console.log("inherited    ", attempt(() => JSON.stringify(new D())));
console.log("erased to any", attempt(() => JSON.stringify(new C() as any)));
console.log("as a field   ", attempt(() => JSON.stringify({ w: new C() })));
console.log("in an array  ", attempt(() => JSON.stringify([new C()])));
console.log("hookless cls ", attempt(() => JSON.stringify(new Plain())));

// §25.5.2.3 — the hook is called ONCE per property. Its answer is
// then serialized as a value, so an answer carrying a `toJSON` of its
// own does not get that one called too. The struct lane calls the
// hook in the compiler (it has the signature and the key), and handing
// the answer back to the whole property operation ran step 2 again.
console.log("once, nested ", attempt(() =>
  JSON.stringify({ p: { toJSON: function () { return { toJSON: function () { return 17; } }; } } })));
console.log("once, gapped ", attempt(() =>
  JSON.stringify({ p: { toJSON: function () { return { toJSON: function () { return 17; } }; } } }, null, 1)));
console.log("once, typed  ", attempt(() => {
  const inner: any = { toJSON: () => 17 };
  return JSON.stringify({ p: { toJSON: () => inner } });
}));

// The receiver reaches the hook.
class R {
  v = 4;
  toJSON() {
    return this.v * 10;
  }
}
console.log("receiver     ", attempt(() => JSON.stringify(new R())));

// A hook answering undefined omits the key (§25.5.2.4 step 8.b).
class U {
  a = 1;
  toJSON() {
    return undefined;
  }
}
console.log("undefined    ", attempt(() => JSON.stringify({ k: new U(), j: 2 })));
// Step 8.a — an element that serializes to nothing is `null` inside
// an array, with and without a gap. Both composite walks gate that
// translation on the static shape, so a value that changes road has
// to change their gate with it.
console.log("undef in arr ", attempt(() => JSON.stringify([new U(), 2])));
console.log("undef gapped ", attempt(() => JSON.stringify([new U()], null, 1)));
console.log("omit gapped  ", attempt(() => JSON.stringify({ k: new U(), j: 2 }, null, 1)));

// Branches that must not move: a Closure-typed slot keeps the
// compiled call, a non-callable slot keeps serializing as data, and
// an object with no hook unfolds as before.
console.log("closure slot ", attempt(() => JSON.stringify({ toJSON: () => 5, a: 1 })));
console.log("number slot  ", attempt(() => JSON.stringify({ toJSON: 3 as any, a: 1 })));
console.log("no hook      ", attempt(() => JSON.stringify({ a: 1, b: "x" })));
console.log("hook + key   ", attempt(() => JSON.stringify({ k: { toJSON: (key: string) => "K" + key } })));
