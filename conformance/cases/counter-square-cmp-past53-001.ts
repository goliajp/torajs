// A square compared against an integer past 2^53: Number
// multiplication rounds the product, so 94906267 * 94906267, exactly
// 9007199515875289, compares equal to 9007199515875288.

let x = 94906267
const n = 9007199515875288
console.log(x * x == n, x * x != n, x * x < n, x * x > n, x * x <= n, x * x >= n)
console.log(n == x * x, n < x * x, n >= x * x)

function le(x: number, n: number): boolean {
  return x * x <= n
}

function lt(x: number, n: number): boolean {
  return n > x * x
}

function sweep(from: number, n: number): string {
  let out = ''
  for (let d = from; d < from + 6; d++) {
    out += (d * d == n ? 'e' : '') + (le(d, n) ? 'l' : 'g') + (lt(d, n) ? 't' : 'f') + ' '
  }
  return out
}

console.log(sweep(94906264, 9007199515875288))
console.log(sweep(94906264, 9007199515875290))
console.log(sweep(-94906269, 9007199515875288))
// a bound past 2^53 that the loop counts up to
function bound(n: number): number {
  let d = 94906260
  while (d * d <= n) d++
  return d
}

console.log(bound(9007199515875288), bound(9007199254740992), bound(1e16))
