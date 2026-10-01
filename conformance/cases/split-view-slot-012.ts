// An alias chain: `let b = last; let c = b` all share one class and
// one block; the class stays views when nothing else touches it.
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
const b = last
let c = b
churn()
console.log(b.join('|'), c.join('|'), c.length, b[0] === last[0])
for (let i = 0; i < lines.length; i++) {
  const parts = lines[i].split(',')
  c = parts
}
churn()
console.log(c.join('|'), last.join('|'))
// a chain whose tail is written: everyone owns
let d: string[] = []
for (let i = 0; i < lines.length; i++) {
  const parts = lines[i].split(',')
  d = parts
}
const e = d
let f = e
f.push('tail' + lines.length)
churn()
console.log(d.join('|'), e.join('|'), f.join('|'), d.length === f.length)
