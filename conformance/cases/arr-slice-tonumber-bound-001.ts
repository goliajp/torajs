// ES §23.1.3.28 — slice's start / end go through
// ToIntegerOrInfinity, which refuses no value.
const xs = [1, 2, 3, 4, 5];
console.log(xs.slice(true).join(","));
console.log(xs.slice(null, 3).join(","));

const vo = { valueOf() { return 1; } };
console.log(xs.slice(vo).join(","));
console.log(xs.slice(0, vo).join(","));
console.log(xs.slice("2").join(","));

// An `any` end may be HOLDING undefined; §23.1.3.28 step 4 reads that
// as len, not as ToNumber's NaN -> 0.
const u: any = undefined;
console.log(xs.slice(1, u).join(","));
console.log(xs.slice(u, 2).join(","));

const boom = { valueOf(): number { throw new Error("boom"); } };
try {
  xs.slice(boom);
} catch (e) {
  console.log("caught " + (e as Error).message);
}
console.log(xs.join(","));
