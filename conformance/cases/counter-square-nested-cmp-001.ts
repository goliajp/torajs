// A square compared against an operand that holds another square
// compare: each compare reads its own square, and the outer one past
// 3037000499, where the integer product would wrap, still compares the
// rounded f64 product.

let x = 3037000500
let y = 1
let m = 5
console.log(x * x < (y * y < m ? 10 : 20))
console.log((y * y < m ? 10 : 20) > x * x)
console.log(x * x == (y * y == 1 ? 7 : 8))
console.log(x * x != (y * y <= m ? 7 : 8))

function nested(a: number, b: number): string {
  let out = ''
  for (let i = 0; i < 4; i++) {
    out += (a * a < (b * b < 100 ? 10 : 20)) + ' '
    out += (a * a >= (b * b > 100 ? 10 : 20)) + ' '
    a = a + 1
    b = b + 7
  }
  return out
}

console.log(nested(3037000498, 1))
console.log(nested(-3037000501, 1))
console.log(nested(3, 9))
