// F0b — index-call receiver semantics (§13.3.6.2 thisValue = base)
// + F0 string leg (§22.1.3.36 String.prototype[Symbol.iterator]).

// builtin method through a runtime string key
const a: any = [1, 2, 3];
const k1: any = "values";
const it1: any = a[k1]();
console.log(it1.next().value, it1.next().value);

// user objlit method — this must bind to the base
const o: any = { i: 21, geti() { return this.i * 2; }, plain(x: number) { return x + 1; } };
const k2: any = "geti";
console.log(o[k2]());
console.log(o["plain"](4));

// growth-relocating builtin through the index call — recv_slot writeback
const xs: any = [1];
xs["push"](2, 3);
console.log(xs.length, xs[2]);

// symbol key — array hand-driven protocol
const src: any = [10, 20];
const it2: any = src[Symbol.iterator]();
let step: any = it2.next();
while (!step.done) { console.log(step.value); step = it2.next(); }

// string @@iterator — short string
const s1: any = "ab";
const its1: any = s1[Symbol.iterator]();
console.log(its1.next().value, its1.next().value, its1.next().done);

// string @@iterator — heap string + for-of over the minted iterator stays consistent
const s2: any = "hello world, this is a heap string";
const its2: any = s2[Symbol.iterator]();
console.log(its2.next().value, its2.next().value);

// value read face — typeof + name
const f: any = s1[Symbol.iterator];
console.log(typeof f, f.name);

// numeric key — ToString lane (§7.1.19 step 3)
const numKeyed: any = { "0": () => "zero" };
console.log(numKeyed[0]());

// negative — a non-method key is a catchable TypeError
try {
  const bad: any = "nope";
  o[bad]();
} catch (e: any) {
  console.log("caught", e instanceof TypeError);
}

// negative — numbers have no @@iterator
try {
  const n: any = 5;
  n[Symbol.iterator]();
} catch (e: any) {
  console.log("caught2", e instanceof TypeError);
}

// StringWrapper inherits the same @@iterator face
const w: any = new String("hi");
const itw: any = w[Symbol.iterator]();
console.log(itw.next().value, itw.next().value, itw.next().done);
