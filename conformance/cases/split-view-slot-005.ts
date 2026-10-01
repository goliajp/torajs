// The rebound binding is handed to a `string[]` parameter: an escape,
// so the class holds owned elements and the callee reads owned strings.
function churn(): void {
  const j: string[] = []
  for (let i = 0; i < 64; i++) j.push('vvvvvvvvv' + i)
}
function count(xs: string[]): number {
  let n = 0
  for (const x of xs) n = n + x.length
  return n
}
function grow(xs: string[]): void {
  xs.push('g' + xs.length)
}
const lines: string[] = []
for (let i = 0; i < 3; i++) lines.push('p' + i + ' qq' + i)
let last: string[] = []
for (let i = 0; i < lines.length; i++) {
  const parts = lines[i].split(' ')
  last = parts
}
churn()
console.log(count(last), last.join('|'))
grow(last)
churn()
console.log(last.join('|'), last.length)
