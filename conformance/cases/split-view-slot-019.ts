// The `Array<string>` spelling of the annotation and an `as string[]`
// on the right of the rebind: both connect the class, so the product
// stays views in every slot.
function churn(): void {
  const j: string[] = []
  for (let i = 0; i < 64; i++) j.push('vvvvvvvvv' + i)
}
const lines: string[] = []
for (let i = 0; i < 3; i++) lines.push('a' + i + ',b' + i + ',c' + i)
let generic: Array<string> = []
for (let i = 0; i < lines.length; i++) {
  const parts = lines[i].split(',')
  generic = parts
}
churn()
console.log(generic.length, generic.join('|'), generic[2])
let cast: string[] = []
for (let i = 0; i < lines.length; i++) {
  const parts = lines[i].split(',')
  cast = parts as string[]
}
churn()
console.log(cast.length, cast.join('|'), cast[0])
let both: Array<string> = []
for (let i = 0; i < lines.length; i++) {
  const parts: Array<string> = lines[i].split(',')
  both = parts as Array<string>
}
churn()
console.log(both.length, both.join('|'), both[1])
