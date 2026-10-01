// The rebound binding is returned: the function's result is typed by
// its own annotation, so the class holds owned elements.
function churn(): void {
  const j: string[] = []
  for (let i = 0; i < 64; i++) j.push('vvvvvvvvv' + i)
}
function lastRow(lines: string[]): string[] {
  let last: string[] = []
  for (let i = 0; i < lines.length; i++) {
    const parts = lines[i].split(',')
    last = parts
  }
  return last
}
const lines: string[] = []
for (let i = 0; i < 3; i++) lines.push('a' + i + ',b' + i + ',c' + i)
const got = lastRow(lines)
churn()
console.log(got.join('|'), got.length, got[2])
got.push('d' + got.length)
console.log(got.join('|'))
