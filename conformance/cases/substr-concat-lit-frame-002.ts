// A split piece joined with a short literal that outlives the loop
// iteration: kept in an array and printed afterwards, next to a
// sibling that is only read in the same loop.
function keep(line: string): string[] {
  const parts = line.split(',')
  const out: string[] = []
  let total = 0
  for (let i = 0; i < parts.length; i = i + 1) {
    const s = parts[i] + ';'
    out.push(s)
    const t = parts[i] + '|'
    total = total + t.charCodeAt(t.length - 1)
  }
  out.push('' + total)
  return out
}

console.log(keep('alpha,beta,gamma').join(''))
console.log(keep('中文,x,' + 'y'.repeat(70)).join(''))
