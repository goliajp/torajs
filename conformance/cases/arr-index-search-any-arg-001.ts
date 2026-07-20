// Any needle into typed-array indexOf / lastIndexOf / includes —
// checker admit paired with emit_compare's needle-Any arm: the
// ELEMENT packs as a (tag, value) pair against the boxed needle,
// so equality stays by-tag strict (§7.2.15 — an any holding "2"
// never matches the number 2) and includes rides the SameValueZero
// kernel (§23.1.3.16 — NaN equals NaN).
const a: any = 2;
const out: number[] = [1, 2, 3];
console.log(out.indexOf(a), out.includes(a), out.lastIndexOf(a));

const s: any = "2";
console.log(out.indexOf(s), out.includes(s));

const miss: any = 9;
console.log(out.indexOf(miss), out.includes(miss));

const strs: string[] = ["p", "qq", "p"];
const sp: any = "p";
console.log(strs.indexOf(sp), strs.lastIndexOf(sp), strs.includes(sp));

const nan: any = NaN;
const fs: number[] = [1.5, NaN, 3.5];
console.log(fs.includes(nan), fs.indexOf(nan));

const fr: any = 2;
console.log(out.indexOf(a, 1), out.indexOf(a, 2), out.lastIndexOf(a, fr));
