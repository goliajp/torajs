// The compile-time literal-descriptor arm only knew how to store an
// AccessorPair for an Any or Arr receiver. Every other receiver fell
// PAST the accessor branch into the data-flags path, where `get` and
// `set` were silently dropped and a property with no value appeared
// in their place. Those receivers decline to the runtime
// ToPropertyDescriptor road now.

function f0(): void {}
Object.defineProperty(f0, "zz", { get: function () { return 7; } });
const d: any = Object.getOwnPropertyDescriptor(f0, "zz");
console.log("fn", typeof d.get, d.value, (f0 as any).zz);

let store = 0;
Object.defineProperty(f0, "ss", {
  get: function () { return store; },
  set: function (v: any) { store = v * 2; },
});
(f0 as any).ss = 21;
console.log("fn set", (f0 as any).ss, store);

// a getter on a function's own `name` / `length` runs — including
// the one that throws, which is what makes `.bind` abrupt
const t: any = Object.defineProperty(function () {}, "name", {
  get: function () { throw new Error("boom"); },
});
try { t.bind(); console.log("no throw"); } catch (e) { console.log("threw", (e as any).message); }
const u: any = Object.defineProperty(function () {}, "length", {
  get: function () { throw new Error("blen"); },
});
try { u.bind(); console.log("no throw2"); } catch (e) { console.log("threw2", (e as any).message); }

// a Map receiver carries an expando bag too, and took the same road
const m = new Map<string, number>();
Object.defineProperty(m, "qq", { get: function () { return "Q"; } });
console.log("map", (m as any).qq);

// the data-descriptor fast path is untouched
Object.defineProperty(f0, "dd", { value: 5, enumerable: true });
console.log("data", (f0 as any).dd, JSON.stringify(Object.keys(f0)));
