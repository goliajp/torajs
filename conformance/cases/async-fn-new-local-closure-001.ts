// `new C()` in a moved async body where `C` is a VALUE — a local
// function expression, or a top-level plain function — constructs
// through the value channel; the machine field holding the result is
// `any`, not the (non-)type `C` (`unknown type C for field o`).
function F(this: any, n: number) { this.n = n; }
async function outer(): Promise<any> {
  const C = function (this: any) { this.v = 3; };
  await null;
  const o = new (C as any)();
  const f = new (F as any)(4);
  return o.v + f.n;
}
outer().then((v: any) => console.log("new-local-closure", v));
