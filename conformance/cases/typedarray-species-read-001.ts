// RFC 20260823-typedarray-substrate @@species knife, second half:
// §7.3.20 steps 5-8 — the constructor OBJECT's @@species is read
// (a getter runs), undefined / null default, and a non-constructor
// species is the step-8 TypeError.
function tryIt(label: string, f: any): void {
  try {
    const r: any = f();
    console.log(label, "ok", r.length !== undefined ? r.length : r.byteLength);
  } catch (e: any) {
    console.log(label, "threw", e.constructor.name);
  }
}
const withCtor = (ctor: any): any => {
  const ta: any = new Int8Array([1, 2, 3]);
  Object.defineProperty(ta, "constructor", { value: ctor, configurable: true });
  return ta;
};

// species undefined / null → default product
const c1: any = {};
c1[Symbol.species] = undefined;
tryIt("species-undef", () => withCtor(c1).slice());
const c2: any = {};
c2[Symbol.species] = null;
tryIt("species-null", () => withCtor(c2).slice(1));

// species is a non-constructor object → TypeError
const c3: any = {};
c3[Symbol.species] = {};
tryIt("species-nonctor", () => withCtor(c3).slice());
const c4: any = {};
c4[Symbol.species] = 7;
tryIt("species-prim", () => withCtor(c4).map((x: any) => x));

// species getter throws → the throw surfaces
const c5: any = {};
Object.defineProperty(c5, Symbol.species, { get: () => { throw new RangeError("sp"); } });
tryIt("species-get-abrupt", () => withCtor(c5).filter(() => true));

// ArrayBuffer face, same walk
const ac: any = {};
ac[Symbol.species] = "neither";
const ab: any = new ArrayBuffer(8);
Object.defineProperty(ab, "constructor", { value: ac, configurable: true });
tryIt("ab-species-nonctor", () => ab.slice(0));
