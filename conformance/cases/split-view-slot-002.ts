// A rebound binding written into with an owned string: the whole alias
// class holds owned elements, and the product is materialized where it
// is declared.
function churn(): void {
  const j: string[] = []
  for (let i = 0; i < 64; i++) j.push('vvvvvvvvv' + i)
}
const lines: string[] = []
for (let i = 0; i < 3; i++) lines.push('p' + i + ' q' + i)
let last: string[] = []
for (let i = 0; i < lines.length; i++) {
  const parts = lines[i].split(' ')
  last = parts
}
last.push('z' + lines.length)
churn()
console.log(last.join('|'), last.length, last[last.length - 1])
// the write happens before the rebind in source order
let early: string[] = []
early.unshift('e' + lines.length)
for (let i = 0; i < lines.length; i++) {
  const parts = lines[i].split(' ')
  early = parts
}
churn()
console.log(early.join('|'), early.length)
