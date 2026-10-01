// Every in-home reader on a view slot: join / indexOf / includes / at
// / JSON.stringify / for-of / slice / map / filter / length / index.
function churn(): void {
  const j: string[] = []
  for (let i = 0; i < 64; i++) j.push('vvvvvvvvv' + i)
}
const lines: string[] = []
for (let i = 0; i < 3; i++) lines.push('m' + i + ' n' + i + ' o' + i)
let last: string[] = []
for (let i = 0; i < lines.length; i++) {
  const parts = lines[i].split(' ')
  last = parts
}
churn()
console.log(last.join('.'), last.indexOf('n2'), last.includes('o2'), last.at(0), last.at(-1))
console.log(JSON.stringify(last), last.length, last[1])
for (const x of last) console.log(x + '!')
console.log(last.slice(1).join('-'), last.map((x: string): string => x + '?').join(''))
console.log(last.filter((x: string): boolean => x !== 'n2').join('-'))
console.log(String(last), last + '', last.toString())
console.log(last.lastIndexOf('m2'), last.concat(['p' + lines.length]).join(','))
