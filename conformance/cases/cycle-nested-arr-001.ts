// RFC 20260706 chunk 575 — an array stored into a container slot
// after the field-store/any-box boundary must be chain-marked so
// the cycle walker can descend through it (pushed-in nested arrays
// were born UNSET and hid the cycle link).
class H {
  grid: H[][] = [];
  tag: number = 0;
}

// 1. cycle through a pushed-in nested array.
{
  let h = new H();
  h.tag = 5;
  let inner: H[] = [];
  inner.push(h);
  h.grid.push(inner);
  console.log(h.tag, h.grid.length, inner.length);
}
Bun.gc(true);

// 2. index-assign twin: replace the nested slot with a fresh array.
{
  let h = new H();
  let inner1: H[] = [];
  h.grid.push(inner1);
  let inner2: H[] = [];
  inner2.push(h);
  h.grid[0] = inner2;
  console.log(h.grid.length);
}
Bun.gc(true);

// 3. live holder survives (external ref keeps the chain black).
let keep = new H();
let inner3: H[] = [];
inner3.push(keep);
keep.grid.push(inner3);
Bun.gc(true);
console.log(keep.tag, keep.grid.length);
console.log("done");
