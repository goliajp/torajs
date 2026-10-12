// A split piece joined with a short literal and then only read: short
// pieces, a piece that fills the pooled payload exactly, pieces past
// it, and pieces of a UTF-16 parent with a Latin-1 or a UTF-16
// literal.
function codes(line: string, sep: string): number {
  const parts = line.split(sep)
  let total = 0
  for (let i = 0; i < parts.length; i = i + 1) {
    const s = parts[i] + '|'
    total = total * 31 + s.charCodeAt(0) + s.charCodeAt(s.length - 1) + s.length
    total = total % 1000000007
  }
  return total
}

function wide(line: string): number {
  const parts = line.split(',')
  let total = 0
  for (let i = 0; i < parts.length; i = i + 1) {
    const s = parts[i] + '中文'
    total = total * 31 + s.charCodeAt(0) + s.charCodeAt(s.length - 2) + s.length
    total = total % 1000000007
  }
  return total
}

console.log(codes('alpha,beta,,gamma,delta', ','))
const fill = 'q'.repeat(63)
const past = 'r'.repeat(64)
const long = 's'.repeat(200)
console.log(codes(fill + ',' + past + ',' + long + ',x', ','))
console.log(codes('中文,ab,é,' + 'ü'.repeat(70), ','))
console.log(wide('中,ab,' + '文'.repeat(30) + ',' + '字'.repeat(31) + ',' + 'x'.repeat(40)))
