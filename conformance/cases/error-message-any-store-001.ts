// `this.<string field> = <any value>` coerces, and the coercion mints
// a string of its own. The store must not retain it a second time —
// the built-in Error constructor takes exactly this shape, so every
// `new Error(msg)` stranded one string cell.
class Box { m: string = ""; }
const strs: any[] = ["s", "", "longer string here", "x"];
for (let i = 0; i < strs.length; i++) {
  const b = new Box();
  b.m = strs[i];
  console.log(i, b.m, b.m.length);
}
// §20.5.1.1 step 3 — the constructors that made it observable
console.log(new Error("boom").message);
console.log(new TypeError("bad").message);
console.log(new RangeError("oob").message);
console.log(new Error().message === "");
console.log(String(new Error("boom")));
// a non-string message takes the ToString arm of the same install
console.log(new Error(42 as any).message);
console.log(new Error(true as any).message);
class Derived extends Error { constructor(m: string) { super(m); } }
console.log(new Derived("sub").message);
try { throw new Error("thrown"); } catch (e) { console.log((e as Error).message); }
// repeated construction must not change what any of them say
for (let i = 0; i < 200; i++) { const e = new Error("loop"); if (e.message !== "loop") console.log("BAD", i); }
console.log("stable");
