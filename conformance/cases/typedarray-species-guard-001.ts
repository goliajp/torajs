// RFC 20260823-typedarray-substrate @@species knife, constructor-
// face half: TypedArraySpeciesCreate begins with Get(O,
// "constructor"), so an instance-installed throwing getter must
// surface from slice / subarray / filter / map, and a primitive
// constructor is the §7.3.20 step-4 TypeError.
function tryIt(label: string, f: any): void {
  try {
    const r: any = f();
    console.log(label, "ok", r.length !== undefined ? r.length : r.byteLength);
  } catch (e: any) {
    console.log(label, "threw", e.constructor.name);
  }
}

const mk = (): any => {
  const ta: any = new Int8Array([1, 2, 3, 4]);
  Object.defineProperty(ta, "constructor", { get: () => { throw new RangeError("poison"); }, configurable: true });
  return ta;
};
tryIt("slice", () => mk().slice(1));
tryIt("subarray", () => mk().subarray(1));
tryIt("map", () => mk().map((x: any) => x));
tryIt("filter", () => mk().filter(() => true));

// toReversed uses TypedArrayCreateSameType — species is NOT read
tryIt("toReversed", () => mk().toReversed());

// explicit undefined defaults; a plain object proceeds to default
const u: any = new Int8Array([9, 8]);
Object.defineProperty(u, "constructor", { value: undefined, configurable: true });
tryIt("undef-ctor", () => u.slice());
const o: any = new Int8Array([7, 6]);
Object.defineProperty(o, "constructor", { value: {}, configurable: true });
tryIt("obj-ctor", () => o.slice(0));

// a primitive constructor is the step-4 TypeError
const p: any = new Int8Array([5, 4]);
Object.defineProperty(p, "constructor", { value: 5, configurable: true });
tryIt("prim-ctor", () => p.slice());

// ArrayBuffer.prototype.slice reads the same face
const ab: any = new ArrayBuffer(8);
Object.defineProperty(ab, "constructor", { get: () => { throw new RangeError("ab-poison"); }, configurable: true });
tryIt("ab-slice", () => ab.slice(0, 4));
const ab2: any = new ArrayBuffer(8);
Object.defineProperty(ab2, "constructor", { value: "nope", configurable: true });
tryIt("ab-prim", () => ab2.slice(0));
