// Object.create proto validation — §20.1.2.2 step 1 (RFC
// 20260712-object-create-define-props chunk 1)
function attempt(label: string, fn: () => any) {
  try {
    fn();
    console.log(label, "no-throw");
  } catch (e) {
    console.log(label, "TypeError:", e instanceof TypeError);
  }
}
// non-object, non-null protos throw
attempt("undefined", () => Object.create(undefined as any));
attempt("number", () => Object.create(42 as any));
attempt("string", () => Object.create("s" as any));
attempt("boolean", () => Object.create(true as any));
// legal protos pass
const n: any = Object.create(null);
n.x = 7;
console.log("null-proto", n.x, Object.keys(n).length);
const o: any = Object.create({});
o.y = 8;
console.log("obj-proto", o.y);
const a: any = Object.create([1, 2]);
console.log("arr-proto", Object.keys(a).length);
// any-typed proto value dispatched at runtime
const dyn: any = { k: 1 };
const d: any = Object.create(dyn);
console.log("any-proto", Object.keys(d).length);
const nul: any = null;
const d2: any = Object.create(nul);
console.log("any-null-proto", Object.keys(d2).length);
const bad: any = 5;
attempt("any-number", () => Object.create(bad));
// owned-temp proto (Call product) — release lane, repeated
for (let i = 0; i < 3; i++) {
  const c: any = Object.create({ v: i });
  console.log("call-proto", i, Object.keys(c).length);
}
