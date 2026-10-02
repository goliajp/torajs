// A two-argument split inside a class method: a limit copies the
// prefix out as owned strings, so the method-local class it is
// rebound into holds owned elements, like the one stored into a
// field.
function churn(): void {
  const j: string[] = []
  for (let i = 0; i < 64; i++) j.push('vvvvvvvvv' + i)
}
class Rows {
  kept: string[] = []
  count = 0
  take(lines: string[]): string {
    let last: string[] = []
    for (let i = 0; i < lines.length; i++) {
      const parts = lines[i].split(',', 2)
      last = parts
      this.count = this.count + parts.length
    }
    churn()
    return last.join('|') + '/' + last.length
  }
  keep(line: string): void {
    const parts = line.split(',', 3)
    this.kept = parts
  }
}
const lines: string[] = []
for (let i = 0; i < 3; i++) lines.push('a' + i + ',b' + i + ',c' + i + ',d' + i)
const r = new Rows()
console.log(r.take(lines), r.count)
r.keep('p,q,r,s' + lines.length)
churn()
console.log(r.kept.join('|'), r.kept.length, r.kept[2])
