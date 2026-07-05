// L3b #4 — Tag::Obj released through `any`: rc-aware class-layout
// walk (__torajs_obj_drop_rc) releases every refcounted field, then
// frees the block. Looped so pool/allocator reuse would expose a
// double-free; dual-owner shapes exercise the dec gate.

// 1) class instance with str + arr fields released through any.
class Box {
  name: string;
  items: string[];
  n: number;
  constructor(name: string) {
    this.name = name;
    this.items = [name + "-a", name + "-b"];
    this.n = 7;
  }
}
for (let i = 0; i < 3; i++) {
  const b: any = new Box("box-heap-string-payload-" + i);
  console.log(b.name);
  console.log(b.n);
}

// 2) nested class field — outer released through any walks into the
//    inner class instance (child slot release-one-reference).
class Inner {
  tag: string;
  constructor(tag: string) {
    this.tag = tag;
  }
}
class Outer {
  inner: Inner;
  label: string;
  constructor(i: number) {
    this.inner = new Inner("inner-heap-string-payload-" + i);
    this.label = "outer-label-" + i;
  }
}
for (let i = 0; i < 3; i++) {
  const o: any = new Outer(i);
  console.log(o.label);
  console.log(o.inner.tag);
}

// 3) typed + any dual owners; any drops first (dec gate — no walk
//    while the typed side lives), typed still fully readable.
const keep = new Box("kept-box-heap-string-payload");
{
  const ka: any = keep;
  console.log(ka.name);
}
console.log(keep.name);
console.log(keep.items[1]);

// 4) anonymous struct released through any — stamped shapes carry a
//    class_layouts entry too (W-J A0/A1), so the runtime walk frees
//    the field strings.
function mkAnon(i: number): any {
  const t = { key: "anon-struct-heap-string-payload-" + i, num: i };
  return t;
}
for (let i = 0; i < 3; i++) {
  const a: any = mkAnon(i);
  console.log(a.key);
  console.log(a.num);
}
