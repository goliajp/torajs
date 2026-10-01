// An element store through the rebound binding (`last[0] = v`) poisons
// the class the same way push does.
function churn(): void {
  const j: string[] = []
  for (let i = 0; i < 64; i++) j.push('vvvvvvvvv' + i)
}
const lines: string[] = []
for (let i = 0; i < 3; i++) lines.push('p' + i + ' q' + i + ' r' + i)
let last: string[] = []
for (let i = 0; i < lines.length; i++) {
  const parts = lines[i].split(' ')
  last = parts
}
last[0] = 'w' + lines.length
last[last.length - 1] = 'v' + lines.length
churn()
console.log(last.join('|'), last.length, last[0], last[2])
// the store goes through the product's own name, after the rebind
let other: string[] = []
for (let i = 0; i < lines.length; i++) {
  const parts = lines[i].split(' ')
  other = parts
  parts[1] = 'm' + i
}
churn()
console.log(other.join('|'), other[1])
