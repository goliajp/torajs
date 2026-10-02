// A string binding aliased and then rebound to a longer string: the
// alias keeps the value it was given, whether it is read, kept, or the
// binding is a module global or a function local.
let g = ''
let seen = 0
for (let i = 0; i < 5; i = i + 1) {
  g = g + 'q'
  const a = g
  g = g + 'r'
  seen = seen + a.length * 100 + a.charCodeAt(a.length - 1)
}
console.log(seen, g)

const kept: string[] = []
let h = ''
for (let i = 0; i < 5; i = i + 1) {
  h = h + 'q'
  const a = h
  h = h + 'r'
  kept.push(a)
}
console.log(kept.join(','), h)

function local(): string {
  let x = ''
  const out: string[] = []
  for (let i = 0; i < 4; i = i + 1) {
    x = x + 'z'
    const a = x
    x = x + 'y'
    out.push(a)
  }
  return out.join(',') + '|' + x
}
console.log(local())
