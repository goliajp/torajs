// RFC 20260706 Phase C (chunk 574) — Arr<Any> joins the cycle walk:
// a class instance holding itself through an any[] field is a cycle
// the collector must reclaim (slots are NaN-box AnyValues; the
// walker filters immediates per slot). Mixed immediates in the same
// array must not confuse the walk.
class Holder {
  xs: any[] = [];
  tag: number = 0;
}

// 1. self-cycle through any[] with mixed immediate slots.
{
  let h = new Holder();
  h.tag = 7;
  h.xs.push(1 as any);
  h.xs.push("str-elem" as any);
  h.xs.push(h as any);
  h.xs.push(3.5 as any);
  console.log(h.tag, h.xs.length);
}
Bun.gc(true);

// 2. two-node cycle: a -> xs -> b -> xs -> a.
{
  let a = new Holder();
  let b = new Holder();
  a.tag = 1;
  b.tag = 2;
  a.xs.push(b as any);
  b.xs.push(a as any);
  console.log(a.tag + b.tag);
}
Bun.gc(true);

// 3. live holder survives collection (external ref keeps it black).
let keep = new Holder();
keep.tag = 42;
keep.xs.push(keep as any);
Bun.gc(true);
console.log(keep.tag);
console.log("done");

// 4. typed Holder[] cycle link (field-store marks ARR_KIND_HEAP;
// exercises the slot-offset fix — the walk previously read the
// props slot as slot 0 and missed the true last element).
class TypedHolder {
  ys: TypedHolder[] = [];
  tag: number = 0;
}
{
  let t = new TypedHolder();
  t.tag = 9;
  t.ys.push(t);
  console.log(t.tag, t.ys.length);
}
Bun.gc(true);
console.log("done2");
