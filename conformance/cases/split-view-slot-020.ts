// An exported `let` rebound to a split product: the export makes the
// binding a module export, and a rebind into it must still agree with
// the product's layout.
function churn(): void {
  const j: string[] = []
  for (let i = 0; i < 64; i++) j.push('vvvvvvvvv' + i)
}
const lines: string[] = []
for (let i = 0; i < 3; i++) lines.push('a' + i + ',b' + i + ',c' + i)
export let last: string[] = []
for (let i = 0; i < lines.length; i++) {
  const parts = lines[i].split(',')
  last = parts
}
churn()
console.log(last.length, last.join('|'), last[2])
export let count = 0
for (let i = 0; i < lines.length; i++) {
  const parts = lines[i].split(',')
  last = parts
  count = count + parts.length
}
churn()
console.log(count, last[0], last.join('+'))
