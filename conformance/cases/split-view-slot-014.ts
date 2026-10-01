// The rebound binding is yielded from a generator: an escape, so the
// class holds owned elements and the consumer reads owned strings.
function churn(): void {
  const j: string[] = []
  for (let i = 0; i < 64; i++) j.push('vvvvvvvvv' + i)
}
function* rows(lines: string[]) {
  let last: string[] = []
  for (let i = 0; i < lines.length; i++) {
    const parts = lines[i].split(',')
    last = parts
    yield last
  }
  yield last
}
const lines: string[] = []
for (let i = 0; i < 3; i++) lines.push('a' + i + ',b' + i)
const got: string[] = []
for (const r of rows(lines)) {
  churn()
  got.push(r.join('|'))
}
console.log(got.join(' '), got.length)
