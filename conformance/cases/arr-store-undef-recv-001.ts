// A store through an array receiver that holds undefined is a
// TypeError raised after the right-hand side runs (§13.15.2 PutValue,
// §7.1.18 ToObject): an element store and a `length` store, through a
// read past the end of an array of arrays, an optional field, and a
// local bound to one. The typed store used to write through the
// undefined cell and crash.
function rhs(tag: string): number {
  console.log('rhs', tag)
  return 1
}

function attempt(tag: string, f: () => void): void {
  try {
    f()
    console.log(tag, 'no throw')
  } catch (e) {
    console.log(tag, e instanceof TypeError)
  }
}

const g: number[][] = [[1]]
attempt('nested elem', () => {
  g[3][0] = rhs('nested elem')
})
attempt('nested length', () => {
  g[3].length = 0
})
const names: string[][] = [['a']]
attempt('nested str', () => {
  names[2][0] = 'x' + rhs('nested str')
})

class Box {
  xs?: number[]
  ss?: string[]
}
const b = new Box()
attempt('optional elem', () => {
  b.xs![1] = rhs('optional elem')
})
attempt('optional length', () => {
  ;(b.xs as number[]).length = 0
})
attempt('optional str', () => {
  b.ss![0] = 'v' + rhs('optional str')
})
attempt('local', () => {
  const ys = g[5]
  ys[0] = rhs('local')
})

// live receivers store as before
g[0][1] = 2
names[0][1] = 'b'
b.xs = [7]
b.xs![1] = 8
console.log(g[0].join(','), names[0].join(','), b.xs!.join(','), g.length)
