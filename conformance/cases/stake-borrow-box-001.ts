// Rotation 185 stake audit — borrowed sources crossing box_to_any
// rc-neutral encodes into owning consumers need a compensating inc.
// Four sites fixed in one lens: any-lane class-field read, typed
// optchain hit, splice Str insert, reduce explicit init. Junk mints
// force freelist reuse so a stolen stake reads back dirty.
class C {
  s: string;
  constructor(n: number) {
    this.s = "minted-payload-" + n;
  }
}
const c = new C(42);
const a: any = c;
let junk: string[] = [];
for (let i = 0; i < 50; i++) {
  const r = a.s;
  junk.push("REUSED-CELL-BY-" + i);
}
console.log(a.s);
console.log(c.s);

type P = { v: string };
function mk(n: number): P | null {
  return { v: "struct-cell-" + n };
}
const p = mk(7);
for (let i = 0; i < 50; i++) {
  const x = p?.v;
  junk.push("STOLEN-SLOT-" + i);
}
console.log(p?.v);

const arr: any[] = [1, 2];
arr.splice(1, 0, "fresh-" + 123);
for (let i = 0; i < 50; i++) {
  junk.push("CLOBBER-" + i);
}
console.log(arr[1]);
const keep = "kept-" + 9;
const arr2: any[] = [];
arr2.splice(0, 0, keep);
console.log(arr2[0], keep);

const xs: any[] = [1, 2, 3];
const seed = { tag: "seed-obj-" + 1 };
const out = xs.reduce((acc: any, v: any) => acc, seed);
console.log(out.tag, seed.tag);
