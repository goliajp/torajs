// `Array<T>.flatMap((T) => Array<U>)` heterogeneous cb return.
// Pre-fix: the method-table entry demanded `(T) => T[]`; a cb
// returning `Array<U>` where U != T was rejected outright.
// Sister to arr-map-heterogeneous-return-001.
const nums = [1, 2, 3];

// Number → String[]
const s: string[] = nums.flatMap(n => [n.toString(), "!"]);
console.log(s.join(","));

// Number → Boolean[]
const bs: boolean[] = nums.flatMap(n => [n > 1, n < 3]);
console.log(bs.map(b => b ? "T" : "F").join(""));

// Number → Number[] identity element then stringified — homogeneous
// return still routes to the method-table arm.
const t: number[] = nums.flatMap(n => [n, n * 10]);
console.log(t.join("|"));

// String → Number[]
const words = ["ab", "cde"];
const lens: number[] = words.flatMap(w => [w.length, w.length * 2]);
console.log(lens.join(","));

// Boolean → String[]
const flags = [true, false];
const labels: string[] = flags.flatMap(b => [b ? "y" : "n", "!"]);
console.log(labels.join(""));
