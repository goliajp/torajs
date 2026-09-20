// §25.1.6.7 — `slice` builds its product with
// Construct(SpeciesConstructor(O, %ArrayBuffer%), « newLen »), not
// with %ArrayBuffer% unconditionally. tr resolved the face, found the
// constructor, and then dropped it (`release_species_product` inside
// the guard), so every one of these answered as if no face existed.

function tag(v: any): string {
  try {
    return String(v);
  } catch (e: any) {
    return "threw:" + (e && e.constructor ? e.constructor.name : "?");
  }
}
function attempt(f: any): string {
  try {
    const r = f();
    return "ok:" + tag(r);
  } catch (e: any) {
    return e && e.constructor ? e.constructor.name : "?";
  }
}

// the face is consulted and its product is the answer
const sc: any = {};
sc[Symbol.species] = function (length: any) {
  return new ArrayBuffer(length);
};
const src: any = new ArrayBuffer(8);
new Uint8Array(src).set([1, 2, 3, 4, 5, 6, 7, 8]);
src.constructor = sc;
const out: any = src.slice(1, 5);
console.log("len", out.byteLength);
console.log("bytes", Array.from(new Uint8Array(out)).join(","));

// a bigger product is legal — the bytes land at the front
const bigger: any = {};
bigger[Symbol.species] = function (_length: any) {
  return new ArrayBuffer(10);
};
const s2: any = new ArrayBuffer(4);
new Uint8Array(s2).set([9, 9, 9, 9]);
s2.constructor = bigger;
const o2: any = s2.slice(0);
console.log("bigger", o2.byteLength, Array.from(new Uint8Array(o2)).join(","));

// the four refusals
console.log("not-a-buffer", attempt(function () {
  const c: any = {};
  c[Symbol.species] = function () { return {}; };
  const b: any = new ArrayBuffer(4);
  b.constructor = c;
  return b.slice(0);
}));
console.log("same-buffer", attempt(function () {
  const c: any = {};
  const b: any = new ArrayBuffer(4);
  c[Symbol.species] = function () { return b; };
  b.constructor = c;
  return b.slice(0);
}));
console.log("too-small", attempt(function () {
  const c: any = {};
  c[Symbol.species] = function () { return new ArrayBuffer(1); };
  const b: any = new ArrayBuffer(4);
  b.constructor = c;
  return b.slice(0);
}));
console.log("face-throws", attempt(function () {
  const c: any = {};
  c[Symbol.species] = function () { throw new RangeError("no"); };
  const b: any = new ArrayBuffer(4);
  b.constructor = c;
  return b.slice(0);
}));

// no face at all: the default product, unchanged
const plain: any = new ArrayBuffer(4);
new Uint8Array(plain).set([7, 7, 7, 7]);
console.log("default", plain.slice(1).byteLength, plain.slice(1).constructor === ArrayBuffer);
