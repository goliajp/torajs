// `[]` as an assignment value: the binding also takes an owned-string
// array, so the class holds owned elements and the slot takes both.
function churn(): void {
  const j: string[] = []
  for (let i = 0; i < 64; i++) j.push('vvvvvvvvv' + i)
}
const lines: string[] = []
for (let i = 0; i < 3; i++) lines.push('a' + i + ',b' + i)
let last: string[] = []
for (let i = 0; i < lines.length; i++) {
  const parts = lines[i].split(',')
  last = parts
  if (i === 1) last = []
}
churn()
console.log(last.join('|'), last.length)
let reset: string[] = []
for (let i = 0; i < lines.length; i++) {
  const parts = lines[i].split(',')
  reset = parts
}
reset = []
churn()
console.log(reset.length, reset.join('|') === '')
// a non-empty literal as the value
let lit: string[] = []
for (let i = 0; i < lines.length; i++) {
  const parts = lines[i].split(',')
  lit = parts
}
lit = ['l' + lines.length]
console.log(lit.join('|'), lit.length)
