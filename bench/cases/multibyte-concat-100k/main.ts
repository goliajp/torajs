// Stress UTF-16 internal representation: concat 100 BMP CJK chars
// 1000 times, then measure the result's code-unit length. Pre-P11.1
// tora would have stored the result as byte-Str so .length would
// have diverged from bun (returning UTF-8 byte count instead of code
// units).
let total = 0
const piece = '中文测试'
const n = 1000
for (let i = 0; i < n; i = i + 1) {
  let acc = ''
  for (let j = 0; j < 100; j = j + 1) {
    acc = acc + piece
  }
  total = total + acc.length
}
console.log(total)
