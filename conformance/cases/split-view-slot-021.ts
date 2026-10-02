// `last.length = n` growing a rebound product: a length store is a
// write through the binding, so the class holds owned elements and
// the grown slots read as holes.
function churn(): void {
  const j: string[] = []
  for (let i = 0; i < 64; i++) j.push('vvvvvvvvv' + i)
}
const lines: string[] = []
for (let i = 0; i < 3; i++) lines.push('a' + i + ',b' + i + ',c' + i)
let last: string[] = []
let width = 0
for (let i = 0; i < lines.length; i++) {
  const parts = lines[i].split(',')
  last = parts
  width = parts.length
}
last.length = width + 3
churn()
console.log(last.length, last[0], last[width + 1], last.join('|'))
console.log(width in last, (width + 1) in last, last.indexOf('b2'))
last.length = 1
churn()
console.log(last.length, last.join('|'), last[0])
last.length = 4
churn()
console.log(last.length, last.join('|'), last[3] === undefined)
last[3] = 'z' + width
console.log(last.join('|'), last.length)
// a length increment is a write through the binding too
let grown: string[] = []
for (let i = 0; i < lines.length; i++) {
  const parts = lines[i].split(',')
  grown = parts
}
grown.length++
churn()
console.log(grown.length, grown[0], grown[width] === undefined, grown.join('|'))
