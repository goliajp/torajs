// A bitwise or shift result over an `any` operand is an int32 / uint32
// whatever the operand held, so a number slot it lands in keeps an
// integer value: the fraction is dropped by ToInt32, not carried.

function h(n: number) {
  return n / 2;
}
function run(raw: any) {
  const n: number = raw.x | 0;
  console.log(n, n / 2);
  console.log(h(raw.x >>> 0), h(raw.neg >>> 0));
  const t: number = ~~raw.neg;
  console.log(t, t / 4);
  let s: number = 0;
  s = raw.x << 3;
  console.log(s / 3);
  const a: number[] = [raw.x & 7, raw.x ^ 1, raw.neg >> 1, raw.big | 0];
  console.log(a.map((v) => v / 2).join(","));
}
run(JSON.parse('{"x":13.75,"neg":-5.5,"big":4294967301.5}'));
