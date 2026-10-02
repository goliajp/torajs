// `%` by an integer constant over a parameter or an array element:
// an integer dividend in [0, 2^53) and every other number give the
// float remainder's answer, including the sign of a zero.
function rem7(x: number): number {
  return x % 7
}
function rem1(x: number): number {
  return x % 1
}
function remBig(x: number): number {
  return x % 9007199254740992
}
function rem1000(x: number): number {
  return x % 1000
}
// the same remainder read as a string and compared with zero
function both(x: number): string {
  const r = x % 1000
  return r.toString() + (r === 0 ? '!' : '')
}

const xs: number[] = [
  2.5,
  -2.5,
  2 ** 60,
  0,
  -0,
  Infinity,
  -Infinity,
  NaN,
  5,
  12,
  -12,
  -7,
  2 ** 53 - 1,
  2 ** 53,
  1e300,
  123456789012,
  0.5,
]
for (let i = 0; i < xs.length; i = i + 1) {
  const x = xs[i]
  console.log(x, rem7(x), rem1(x), remBig(x), rem1000(x), 1 / rem7(x), both(x))
}
console.log(rem7(Number('14')), rem7(Number('-14')), 1 / rem7(Number('-14')), 1 / rem7(Number('-0')))
console.log(remBig(Number('9007199254740993')), rem1(Number('9007199254740993')), rem1000(Number('-1e21')))
