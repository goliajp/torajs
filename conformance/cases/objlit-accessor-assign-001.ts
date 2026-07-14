// RFC 20260714-objlit-accessor blade 8 — Object.assign walks PROPERTIES,
// not layout slots. ES §20.1.2.1 step 4.c.ii reads every source key with
// [[Get]] and writes the target with [[Set]], so an accessor on either
// side is reached through its own half. tr used to compare the two layouts
// and copy slot-by-slot: an accessor source was rejected outright, and a
// get-only target got the SOURCE's getter closure copied over it.

// source getter -> plain data target: the getter ANSWERS the value.
const src = {
  a: 1,
  get v(): number {
    return this.a + 1;
  },
};
const dst = { a: 0, v: 0 };
Object.assign(dst, src);
console.log(dst.a, dst.v);

// data source -> setter target: the write goes THROUGH the setter.
const sink = {
  _v: 0,
  set v(x: number) {
    this._v = x * 10;
  },
  get v(): number {
    return this._v;
  },
};
Object.assign(sink, { _v: 0, v: 5 });
console.log(sink._v, sink.v);

// getter -> setter, with a heap value: the getter's result is OWNED (a
// call answers +1) and the setter takes it as a borrowed arg, so the
// caller releases it afterwards.
const strSrc = {
  s: "x",
  get g(): string {
    return this.s + "!";
  },
};
const strDst = {
  s: "",
  _g: "",
  set g(v: string) {
    this._g = v + "?";
  },
  get g(): string {
    return this._g;
  },
};
Object.assign(strDst, strSrc);
console.log(strDst.s, strDst.g);

// get-only target: ES §10.1.9 — a strict-mode write of a property with a
// [[Get]] and no [[Set]] is a TypeError (a module always is strict).
const roDst = {
  a: 0,
  get v(): number {
    return 0;
  },
};
try {
  Object.assign(roDst, src);
} catch (e) {
  console.log((e as Error).message);
}
// the [[Get]] half still ran before the throw, and `a` was copied first.
console.log(roDst.a, roDst.v);

// §20.1.2.1 is a SHALLOW copy — the target gets the source's own array,
// not a clone of it. tr used to deep-clone every Arr field.
const arrSrc = { arr: [1, 2] };
const arrDst = { arr: [0] };
Object.assign(arrDst, arrSrc);
arrSrc.arr.push(3);
console.log(arrDst.arr.length, arrSrc.arr.length);

// N sources, left-to-right: the last one wins, and each source's getter
// is called once.
let calls = 0;
const s1 = {
  n: 0,
  get k(): number {
    calls += 1;
    return 10;
  },
};
const s2 = {
  n: 0,
  get k(): number {
    calls += 1;
    return 20;
  },
};
const merged = { n: 0, k: 0 };
Object.assign(merged, s1, s2);
console.log(merged.k, calls);
