// Function-local and top-level shapes: a local class in a function, a
// top-level class, and a top-level binding a named function reads
// (promoted to a data global).
function churn(): void {
  const j: string[] = []
  for (let i = 0; i < 64; i++) j.push('vvvvvvvvv' + i)
}
function local(lines: string[]): string {
  let last: string[] = []
  for (let i = 0; i < lines.length; i++) {
    const parts = lines[i].split(',')
    last = parts
  }
  churn()
  return last.join('|') + '/' + last.length
}
const lines: string[] = []
for (let i = 0; i < 3; i++) lines.push('a' + i + ',b' + i)
console.log(local(lines))
let top: string[] = []
for (let i = 0; i < lines.length; i++) {
  const parts = lines[i].split(',')
  top = parts
}
churn()
console.log(top.join('|'), top.length)
let seen: string[] = []
function peek(): string { return seen.join('+') + '/' + seen.length }
for (let i = 0; i < lines.length; i++) {
  const parts = lines[i].split(',')
  seen = parts
}
churn()
console.log(peek(), seen[0])
// a top-level product read from a function body, rebound into a local
const fixed: string[] = ('u,v,w' + lines.length).split(',')
function readFixed(): string { return fixed.join('-') }
let into: string[] = []
into = fixed
churn()
console.log(readFixed(), into.join('|'), into.length)
