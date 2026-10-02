// Two functions each declaring their own `last`: one writes into it
// after the rebind, the other only reads. Alias classes are keyed by
// name across the whole program, so the written one poisons both and
// each holds owned elements; the outputs are the same either way.
function churn(): void {
  const j: string[] = []
  for (let i = 0; i < 64; i++) j.push('vvvvvvvvv' + i)
}
function dirty(lines: string[]): string {
  let last: string[] = []
  for (let i = 0; i < lines.length; i++) {
    const parts = lines[i].split(',')
    last = parts
  }
  last.push('tail' + lines.length)
  churn()
  return last.join('|') + '/' + last.length
}
function clean(lines: string[]): string {
  let last: string[] = []
  for (let i = 0; i < lines.length; i++) {
    const parts = lines[i].split(',')
    last = parts
  }
  churn()
  return last.join('|') + '/' + last.length + '/' + last[1]
}
const lines: string[] = []
for (let i = 0; i < 3; i++) lines.push('a' + i + ',b' + i + ',c' + i)
console.log(dirty(lines))
console.log(clean(lines))
// a class spelled with other names beside them stays views (a `parts`
// here would join the poisoned class through the shared name)
function other(lines: string[]): string {
  let tail: string[] = []
  for (let i = 0; i < lines.length; i++) {
    const cols = lines[i].split(',')
    tail = cols
  }
  churn()
  return tail.join('+') + '/' + tail.length
}
console.log(other(lines))
