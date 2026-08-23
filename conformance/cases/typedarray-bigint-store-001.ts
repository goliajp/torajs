// An owned-temp rhs into a consuming element store — the
// assignment-expression stake must be taken BEFORE the kernel
// releases the transferred pair (the BigInt64Array UAF).
const w = new BigInt64Array(4);
w[0] = 5n;
w[1] = BigInt(2);
w[2] = 4n + 3n;
let i = 3;
w[3] = BigInt(2 * i);
console.log(w[0], w[1], w[2], w[3]);
const ta = new BigInt64Array(4);
const alias = new BigInt64Array(ta.buffer);
for (let k = 0; k < 4; k++) alias[k] = BigInt(2 * k);
console.log(ta[0], ta[1], ta[2], ta[3]);
function isTwoOrFour(n: any) { return n == 2 || n == 4; }
const f: any = Array.prototype.find;
console.log(Number(f.call(ta, isTwoOrFour)));
console.log(Number(ta.find(isTwoOrFour)));
const anyArr: any = [0];
anyArr[0] = 7n + 1n;
console.log(anyArr[0]);
