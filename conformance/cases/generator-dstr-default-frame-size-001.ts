// A generator whose parameter list destructures with defaults: the
// state machine spills the pattern's bindings into the frame class
// after the frame's factory has been lowered, so the factory's
// allocation constant was read from a shorter layout than the stores
// address. The frame must be allocated at its final row, and its
// release must size the cell by what the cell says, not by the early
// constant.
function* f([{ x, y, z } = { x: 44, y: 55, z: 66 }] = [{ x: 11, y: 22, z: 33 }]) {
  yield x
  yield y
  yield z
}
function* g([{ u: v, w: x, y: z } = { u: 444, w: 555, y: 666 }] = [{ u: 777, w: 888, y: 999 }]) {
  yield v
  yield x
  yield z
}
function* h([{ x, y, z } = { x: 1, y: 2, z: 3 }]) {
  let acc = 0
  acc += x
  yield acc
  acc += y
  yield acc
  acc += z
  yield acc
}

const out: number[] = []
for (const v of f()) out.push(v)
for (const v of f([])) out.push(v)
for (const v of f([{ x: 7, y: 8, z: 9 }])) out.push(v)
for (const v of g()) out.push(v)
for (const v of g([])) out.push(v)
for (const v of h([])) out.push(v)
for (const v of h([{ x: 10, y: 20, z: 30 }])) out.push(v)
console.log(out.join(','))

// many frames in flight, released at different depths, so a frame
// freed by a wrong size class would surface as a clobbered neighbour
let total = 0
const live = [f([{ x: 0, y: 0, z: 0 }])]
for (let i = 1; i < 2000; i++) {
  const it = f([{ x: i, y: i * 2, z: i * 3 }])
  total += it.next().value
  if (i % 3 === 0) live.push(it)
}
for (const it of live) total += it.next().value
console.log(total, live.length)
