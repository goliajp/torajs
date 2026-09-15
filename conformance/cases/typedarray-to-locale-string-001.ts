// §23.2.3.32 `%TypedArray%.prototype.toLocaleString`.
//
// The same algorithm as §23.1.3.32 over the typed array's length:
// every element is Invoke'd with "toLocaleString" and the results are
// joined with ",". Before this existed the read fell through to
// `Object.prototype.toLocaleString`, which is `Invoke(this,
// "toString")` — the output looked the same for plain numbers, but
// the element hook never ran and a non-typed-array `this` never threw.

const TA: any = Object.getPrototypeOf(Uint8Array.prototype);

// the property itself
const d = Object.getOwnPropertyDescriptor(TA, "toLocaleString")!;
console.log(typeof d.value, d.writable, d.enumerable, d.configurable);
console.log(d.value.name, d.value.length);
console.log(TA.toLocaleString === (Uint8Array.prototype as any).toLocaleString);
console.log(TA.toLocaleString === (new Float32Array(1) as any).toLocaleString);
console.log(TA.toLocaleString === Array.prototype.toLocaleString);
console.log(Object.prototype.hasOwnProperty.call(Uint8Array.prototype, "toLocaleString"));

// plain rendering across element types
console.log(new Uint8Array([1, 2, 3]).toLocaleString());
console.log(new Int8Array([-1, 0, 127]).toLocaleString());
console.log(new Float64Array([1.5, 0, 42]).toLocaleString());
console.log(JSON.stringify(new Float32Array(0).toLocaleString()));
console.log(new BigInt64Array([-1n, 0n, 7n]).toLocaleString());
console.log(new BigUint64Array([5n]).toLocaleString());

// each Number element invokes Number.prototype.toLocaleString
const numOrig = Number.prototype.toLocaleString;
let numCalls = 0;
(Number.prototype as any).toLocaleString = function (this: number) {
  numCalls++;
  return "n" + this.valueOf();
};
console.log(new Uint16Array([10, 20, 30]).toLocaleString(), numCalls);
console.log(JSON.stringify(new Int32Array(0).toLocaleString()), numCalls);
(Number.prototype as any).toLocaleString = numOrig;

// each BigInt element invokes BigInt.prototype.toLocaleString
const bigOrig = BigInt.prototype.toLocaleString;
let bigCalls = 0;
(BigInt.prototype as any).toLocaleString = function (this: bigint) {
  bigCalls++;
  return "b" + this.valueOf();
};
console.log(new BigInt64Array([1n, 2n]).toLocaleString(), bigCalls);
(BigInt.prototype as any).toLocaleString = bigOrig;

// the hook's result goes through ToString
(Number.prototype as any).toLocaleString = function () {
  return 7;
};
console.log(new Uint8Array([1, 2]).toLocaleString());
(Number.prototype as any).toLocaleString = numOrig;

// a throwing hook propagates, and stops the walk
let seen = 0;
(Number.prototype as any).toLocaleString = function () {
  seen++;
  if (seen === 2) throw new RangeError("hook");
  return "ok";
};
try {
  new Uint8Array([1, 2, 3]).toLocaleString();
  console.log("no throw");
} catch (e) {
  console.log((e as Error).constructor === RangeError, (e as Error).message, seen);
}
(Number.prototype as any).toLocaleString = numOrig;

// a hook that forwards to the original through a Number wrapper
// `this` (what a sloppy-mode hook receives) runs the builtin body
// once, not the hook again
const wrapped: any = new Number(6);
(Number.prototype as any).toLocaleString = function () {
  return "w" + numOrig.call(wrapped);
};
console.log(new Uint8Array([1, 2]).toLocaleString());
(Number.prototype as any).toLocaleString = numOrig;

// the brand check
for (const recv of [{}, [1, 2], 1, "x", new ArrayBuffer(4)]) {
  try {
    TA.toLocaleString.call(recv);
    console.log("no throw");
  } catch (e) {
    console.log((e as Error).constructor === TypeError);
  }
}

// the prototype objects themselves are not typed arrays, so calling
// the method on them fails the same check — for `toString` too,
// which the %Object.prototype% surface used to answer instead
const U8P: any = Uint8Array.prototype;
for (const f of [
  () => TA.toLocaleString(),
  () => U8P.toLocaleString(),
  () => TA.toString(),
  () => (Float64Array.prototype as any).toString(),
]) {
  try {
    console.log("no throw", f());
  } catch (e) {
    console.log((e as Error).constructor === TypeError);
  }
}
console.log(typeof TA.valueOf(), Object.prototype.toString.call(TA));

// back to normal after the patches are gone
console.log(new Uint8Array([4, 5]).toLocaleString());
