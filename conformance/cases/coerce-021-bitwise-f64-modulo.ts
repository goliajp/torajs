// §7.1.6 ToInt32 on a run-time f64 is modulo 2^32 of the truncated
// value, never a saturation: `1e20 | 0` is 1661992960, `Infinity | 0`
// and `2^63 | 0` are 0. The operands are parsed at run time so no
// constant fold answers for the conversion.

const strs = [
  "1e20",
  "-1e20",
  "Infinity",
  "-Infinity",
  "NaN",
  "9223372036854775808",
  "-9223372036854775808",
  "4294967296.5",
  "-0.9",
  "2147483648",
  "-2147483649",
  "1e300",
  "4294967295.7",
];

for (let i = 0; i < strs.length; i++) {
  const x = Number(strs[i]);
  console.log(x | 0, x & 0xff, x >>> 0, x >> 3, ~x, x << 4);
}

const ints = ["1e20", "4294967296", "99999999999", "2147483648", "-2147483649", "abc", "42"];
for (let i = 0; i < ints.length; i++) {
  console.log(parseInt(ints[i], 10) | 0, parseInt(ints[i], 10) >>> 0);
}

for (let i = 0; i < strs.length; i++) {
  const x = Number(strs[i]);
  console.log(Math.imul(x, 3), Math.clz32(x));
}
