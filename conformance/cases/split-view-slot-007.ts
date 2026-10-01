// The rebound binding is captured by a closure: the class holds owned
// elements (the closure body is not walked).
function churn(): void {
  const j: string[] = []
  for (let i = 0; i < 64; i++) j.push('vvvvvvvvv' + i)
}
const lines: string[] = []
for (let i = 0; i < 3; i++) lines.push('a' + i + ' b' + i)
let last: string[] = []
for (let i = 0; i < lines.length; i++) {
  const parts = lines[i].split(' ')
  last = parts
}
const show = (): string => last.join('|') + '/' + last.length
churn()
console.log(show())
const add = (s: string): void => { last.push(s) }
add('c' + lines.length)
churn()
console.log(show(), last[last.length - 1])
