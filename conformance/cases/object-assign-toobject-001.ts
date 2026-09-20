// §20.1.2.1 step 1 is ToObject(target), so a primitive target means
// the property writes land on a WRAPPER and the wrapper is what the
// call answers. Handing the primitive back is a wrong value, and no
// fixture here had a primitive target, so the gate could not see it.
function t(tag: string, f: () => unknown): void {
  try {
    console.log(tag, "ok", f());
  } catch (e: any) {
    console.log(tag, "throw", e.constructor.name);
  }
}

const n: any = 1;
const r1: any = Object.assign(n, {});
console.log("number:", typeof r1, r1.valueOf());

const b: any = true;
const r2: any = Object.assign(b, {});
console.log("boolean:", typeof r2, r2.valueOf());

// The writes go onto the wrapper, which is why this one used to
// throw: a bare primitive has nowhere to put `a`.
const s: any = "test";
const r3: any = Object.assign(s, { a: 1 });
console.log("string:", typeof r3, r3.valueOf(), r3.a);

const sym: any = Symbol("s");
console.log("symbol:", typeof Object.assign(sym, {}));

// ToObject's throwing arm — this is where it differs from
// §20.1.1.1 `Object(v)`, which mints a fresh `{}` instead.
const nul: any = null;
t("null", () => Object.assign(nul, { a: 1 }));
const und: any = undefined;
t("undefined", () => Object.assign(und, { a: 1 }));

// An object target keeps its identity (step 3), and the fresh-object
// idiom is unaffected.
const obj: any = { x: 1 };
const r4: any = Object.assign(obj, { y: 2 });
console.log("identity:", r4 === obj, r4.x, r4.y);
console.log("fresh:", JSON.stringify(Object.assign({}, { a: 1 }, { b: 2 })));
