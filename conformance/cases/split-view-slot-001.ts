// A binding rebound to a split product in a loop (`last = parts`) and
// read after the loop. The two bindings only ever hand the product to
// each other, so both carry it as views: nothing is materialized at
// `parts`' declaration, and `last` holds the last row's block.
function churn(): void {
  const j: string[] = []
  for (let i = 0; i < 64; i++) j.push('vvvvvvvvv' + i)
}
const lines: string[] = []
for (let i = 0; i < 4; i++) lines.push('a' + i + ',bb' + i + ',ccc' + i)
let total = 0
let last: string[] = []
for (let i = 0; i < lines.length; i++) {
  const parts = lines[i].split(',')
  for (let k = 0; k < parts.length; k++) total = total + parts[k].length
  last = parts
}
churn()
console.log(total, last.length, last[last.length - 1], last[0])
console.log(last.join('|'))
// the loop never runs: the slot keeps the empty block it declared
let none: string[] = []
for (let i = 0; i < 0; i++) {
  const parts = lines[i].split(',')
  none = parts
}
console.log(none.length, none.join('|') === '')
// a const product rebound from inside a nested block
let deep: string[] = []
{
  const inner = ('x,y,z' + lines.length).split(',')
  deep = inner
}
churn()
console.log(deep.length, deep[2], deep.join('+'))
