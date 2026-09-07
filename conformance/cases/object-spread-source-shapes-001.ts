// §7.3.25 CopyDataProperties takes any source: undefined and null
// contribute nothing (step 1), a Number / Boolean / BigInt / Symbol /
// Map / Set / function has no own enumerable key, a String
// contributes its index properties, an Array its elements. The
// checker admitted a Struct and `any` and rejected the rest, so the
// receiver's own spelling decided whether the program compiled.
const nul = null;
const und = undefined;
const num = 5;
const bool = true;
const big = 10n;
const str = "ab";
const arr = [1, 2];
const map = new Map([[1, 2]]);
const set = new Set([1]);
const fn = () => 1;
const sym = Symbol("s");
const struct = { a: 1 };

console.log(JSON.stringify({ ...nul, z: 9 }));
console.log(JSON.stringify({ ...und, z: 9 }));
console.log(JSON.stringify({ ...num, z: 9 }));
console.log(JSON.stringify({ ...bool, z: 9 }));
console.log(JSON.stringify({ ...big, z: 9 }));
console.log(JSON.stringify({ ...str, z: 9 }));
console.log(JSON.stringify({ ...arr, z: 9 }));
console.log(JSON.stringify({ ...map, z: 9 }));
console.log(JSON.stringify({ ...set, z: 9 }));
console.log(JSON.stringify({ ...fn, z: 9 }));
console.log(JSON.stringify({ ...sym, z: 9 }));
console.log(JSON.stringify({ ...struct, z: 9 }));
console.log(JSON.stringify({ ...null, ...undefined, ..."hi", a: 1 }));
