// A split call directly on the right of the rebind (`last =
// s.split(',')`, no intermediate binding): the class has no alias
// edge to connect, so the binding holds owned elements and each fresh
// product is materialized at the assignment.
function churn(): void {
  const j: string[] = []
  for (let i = 0; i < 64; i++) j.push('vvvvvvvvv' + i)
}
const lines: string[] = []
for (let i = 0; i < 4; i++) lines.push('a' + i + ',bb' + i + ',ccc' + i)
let last: string[] = []
for (let i = 0; i < lines.length; i++) last = lines[i].split(',')
churn()
console.log(last.length, last[0], last[last.length - 1], last.join('|'))
let two: string[] = []
for (let i = 0; i < lines.length; i++) two = lines[i].split(',', 2)
churn()
console.log(two.length, two.join('|'))
// the same shape rebinding from both a direct split and an alias
let mix: string[] = []
for (let i = 0; i < lines.length; i++) {
  const parts = lines[i].split(',')
  mix = i % 2 === 0 ? parts : lines[i].split(',', 1)
}
churn()
console.log(mix.length, mix.join('|'))
