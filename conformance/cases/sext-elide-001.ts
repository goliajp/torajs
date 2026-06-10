// sext-elide pass — ToInt32 semantics must hold with redundant
// sext-pair elimination active. Covers the popcount hot-loop shape,
// the INT32_MIN - 1 wrap (the case an unsound elide of the sub-side
// pair would break), bitwise chains, and const operands.

function popcount32(x: number): number {
  let n: number = x;
  let count: number = 0;
  while (n !== 0) {
    n = n & (n - 1);
    count = count + 1;
  }
  return count;
}

// INT32_MIN: n - 1 must ToInt32-wrap (-2147483649 → 2147483647)
console.log(popcount32(-2147483648));
console.log(popcount32(2147483647));
console.log(popcount32(0));
console.log(popcount32(123456789));
console.log(popcount32(-1));

// bitwise chains — transposed pairs feed downstream normalizations
let a: number = 305419896; // 0x12345678
let b: number = -559038737; // 0xDEADBEEF as int32
let c: number = 65535;
console.log((a & b) | c);
console.log((a ^ b) & ~c);
console.log(((a | b) ^ (b & c)) >>> 3);
console.log((a & b & c) << 1);
console.log((-2147483648 & -1) >> 31);
console.log(~(a | 0));
console.log((b ^ -1) & (a | 1));

// loop-carried mixing — R1 fires inside the loop, result feeds shifts
function mix(x: number, y: number): number {
  let h: number = x;
  let i: number = 0;
  while (i < 16) {
    h = (h ^ y) & (h | 3);
    h = h >> 1;
    i = i + 1;
  }
  return h;
}
console.log(mix(-2147483648, 2147483647));
console.log(mix(987654321, -123456789));
