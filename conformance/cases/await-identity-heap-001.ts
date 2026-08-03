// `await <non-promise heap value>` is identity per §27.7.5.1 — and
// the awaited binding must be INDEPENDENT: rotation 288 fixed the
// runtime identity arm to mint the +1 stake the call site releases,
// so neither the source binding nor the result dangles.
async function f() {
  const s: any = Symbol('x');
  const t: any = await s;
  console.log(typeof t);
  console.log(t === s);
  console.log(s.description);
  const o: any = { a: 1 };
  const p: any = await o;
  console.log(p === o);
  console.log(p.a);
  console.log(o.a);
}
f();
