// The parent string is rebound (and its old text freed) after the
// views were taken; the views hold their own reference to the parent,
// so reading through `last` afterwards still answers the old text.
function churn(): void {
  const j: string[] = []
  for (let i = 0; i < 64; i++) j.push('vvvvvvvvv' + i)
}
let line = 'alpha,beta,gamma' + '!'
let last: string[] = []
for (let i = 0; i < 3; i++) {
  const parts = line.split(',')
  last = parts
  line = 'delta' + i + ',eps' + i
}
churn()
console.log(line, last.length, last.join('|'), last[0])
// the parent is an array element that is overwritten
const lines: string[] = ['one,two' + '!', 'three,four' + '!']
let keep: string[] = []
for (let i = 0; i < lines.length; i++) {
  const parts = lines[i].split(',')
  keep = parts
  lines[i] = 'gone' + i
}
churn()
console.log(lines.join(' '), keep.join('|'), keep[1])
