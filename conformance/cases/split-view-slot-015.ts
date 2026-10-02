// `last.length = n` on a rebound binding: a length store is a write
// through the binding, so the class holds owned elements and the
// truncation drops the block's cells.
function churn(): void {
  const j: string[] = []
  for (let i = 0; i < 64; i++) j.push('vvvvvvvvv' + i)
}
const lines: string[] = []
for (let i = 0; i < 3; i++) lines.push('a' + i + ',b' + i + ',c' + i)
let last: string[] = []
for (let i = 0; i < lines.length; i++) {
  const parts = lines[i].split(',')
  last = parts
}
console.log(last.length, last.join('|'))
last.length = 1
churn()
console.log(last.length, last.join('|'), last[0])
last.length = 0
churn()
console.log(last.length, last.join('|') === '')
