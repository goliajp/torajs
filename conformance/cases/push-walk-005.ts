// The string the loop walks is rebound in the body, so its length at
// entry bounds nothing; the loop runs to the longer string's end.
function grow(start: string): number[] {
  const out: number[] = []
  let s = start
  let added = 0
  let i = 0
  while (i < s.length) {
    const c = s.charCodeAt(i)
    if (added < 5) {
      s = s + 'z'
      added = added + 1
    }
    out.push(c)
    i = i + 1
  }
  return out
}
function shrink(start: string): number {
  const out: number[] = []
  let s = start
  let i = 0
  while (i < s.length) {
    out.push(s.charCodeAt(i))
    if (i === 1) s = 'ab'
    i = i + 1
  }
  return out.length
}
console.log(grow('ab').join(','), shrink('abcdef'))
