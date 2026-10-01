// A conditional rebind: both branches view products stay views; one
// branch an owned array poisons the class.
function churn(): void {
  const j: string[] = []
  for (let i = 0; i < 64; i++) j.push('vvvvvvvvv' + i)
}
const lines: string[] = []
for (let i = 0; i < 4; i++) lines.push('a' + i + ',b' + i)
let pick: string[] = []
for (let i = 0; i < lines.length; i++) {
  const even = lines[i].split(',')
  const odd = (lines[i] + ',o').split(',')
  pick = i % 2 === 0 ? even : odd
}
churn()
console.log(pick.join('|'), pick.length)
let mixed: string[] = []
const owned: string[] = ['x' + lines.length, 'y' + lines.length]
for (let i = 0; i < lines.length; i++) {
  const parts = lines[i].split(',')
  mixed = i === 1 ? owned : parts
}
mixed.push('z' + lines.length)
churn()
console.log(mixed.join('|'), owned.join('|'))
// nullish and logical forms between class members
let viaNullish: string[] = []
let viaOr: string[] = []
for (let i = 0; i < lines.length; i++) {
  const parts = lines[i].split(',')
  viaNullish = parts ?? viaNullish
  viaOr = viaNullish || parts
}
churn()
console.log(viaNullish.join('|'), viaOr.join('|'))
