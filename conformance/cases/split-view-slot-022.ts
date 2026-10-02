// An `any` sink beside the rebind: `let v: any = parts` widens the
// annotated member of the class to an `any`-element slot, while the
// unannotated product keeps its owned-string layout. The rebind shares
// the block into the widened slot, whose readers rebox each element.
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
  let v: any = parts
  v = parts
  console.log(v.length, v[1], last[0])
}
churn()
console.log(last.length, last.join('|'), last[2], typeof last[0])
// the same class with the member read from a named function body
let seen: string[] = []
function peek(): string { return seen.join('+') + '/' + seen.length }
for (let i = 0; i < lines.length; i++) {
  const parts = lines[i].split(',')
  seen = parts
  const w: any = parts
  console.log(w[0], peek())
}
churn()
console.log(peek(), seen[1])
// an `any[]` slot rebound to the product outright
let anyElems: any[] = []
for (let i = 0; i < lines.length; i++) {
  const parts = lines[i].split(',')
  anyElems = parts
}
churn()
console.log(anyElems.length, anyElems[0], JSON.stringify(anyElems), typeof anyElems[2])
anyElems.push('d' + lines.length)
console.log(anyElems.length, anyElems[3], anyElems.join('|'))
