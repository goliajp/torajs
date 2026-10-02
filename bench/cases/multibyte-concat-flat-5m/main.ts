// Sibling of multibyte-concat-5m whose result escapes by content, not
// only by length: reading the last code unit makes a runtime that keeps
// `acc` as an unflattened rope build the string it was asked for, so the
// two cells together show the cost of the concatenation itself.
let total = 0
const piece = '中文测试'
const n = parseInt(process.argv[2], 10)
const pieces = parseInt(process.argv[3], 10)
for (let i = 0; i < n; i = i + 1) {
  let acc = ''
  for (let j = 0; j < pieces; j = j + 1) {
    acc = acc + piece
  }
  total = total + acc.length + acc.charCodeAt(acc.length - 1)
}
console.log(total)
