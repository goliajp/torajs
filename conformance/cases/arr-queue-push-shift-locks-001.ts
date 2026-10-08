// a push / shift queue whose length lives in a register across the
// loop: every lock, a growth past capacity, the empty shift and an
// aliased length write must still see (and leave) the true length

function drain(q: number[], n: number, cap: number): number {
  let total = 0
  for (let i = 0; i < n; i = i + 1) {
    q.push(i)
    if (q.length > cap) {
      total = total + q.shift()!
    }
  }
  while (q.length > 0) {
    total = total + q.shift()!
  }
  return total
}

function tryDrain(label: string, q: number[], n: number, cap: number): void {
  try {
    const t = drain(q, n, cap)
    console.log(label, "ok", t, q.length)
  } catch (e) {
    console.log(label, "threw", e instanceof TypeError, q.length, q[0], q[q.length - 1])
  }
}

// a queue the function allocates: its length stays in a register
// through the push's slow arm (a growth, a fold of the deque head
// back to slot 0) and the shift
function local(n: number, cap: number): number {
  const q: number[] = []
  let total = 0
  for (let i = 0; i < n; i = i + 1) {
    q.push(i)
    if (q.length > cap) {
      total = total + q.shift()!
    }
  }
  const left = q.length
  while (q.length > 0) {
    total = total + q.shift()!
  }
  return total * 1000 + left
}
console.log("local", local(1000, 16), local(300, 0), local(5, 40), local(0, 3))

function localPrefilled(n: number): number {
  const q: number[] = [7, 8, 9]
  let total = 0
  for (let i = 0; i < n; i = i + 1) {
    q.push(i)
    q.push(i + 1)
    total = total + q.shift()!
  }
  return total * 1000 + q.length
}
console.log("local-prefilled", localPrefilled(100))

// plain: a growth past every capacity step, the queue folding back
tryDrain("plain", [], 1000, 16)
tryDrain("wide", [], 100, 64)
tryDrain("prefilled", [7, 8, 9], 40, 5)

// locked before the loop: the first push refuses
const frozen: number[] = [1, 2, 3]
Object.freeze(frozen)
tryDrain("frozen", frozen, 10, 2)
const sealed: number[] = [4, 5]
Object.seal(sealed)
tryDrain("sealed", sealed, 10, 1)
const fixed: number[] = [6]
Object.preventExtensions(fixed)
tryDrain("nonext", fixed, 10, 0)

// a non-extensible queue that only shifts: shift still works
function shiftAll(q: number[]): number {
  let total = 0
  while (q.length > 0) {
    total = total + q.shift()!
  }
  return total
}
const ne: number[] = [1, 2, 3, 4]
Object.preventExtensions(ne)
console.log("nonext-shift", shiftAll(ne), ne.length)

// the empty shift answers undefined and leaves the length at 0
const empty: number[] = []
let misses = 0
for (let i = 0; i < 5; i = i + 1) {
  if (empty.shift() === undefined) misses = misses + 1
}
console.log("empty", misses, empty.length)

// push and shift alternating on a one-slot queue, then a lock mid-run
function alternate(q: number[], n: number, lockAt: number): number {
  let total = 0
  for (let i = 0; i < n; i = i + 1) {
    if (i === lockAt) Object.freeze(q)
    q.push(i)
    total = total + q.shift()!
  }
  return total
}
const alt: number[] = []
try {
  console.log("alt", alternate(alt, 50, 30))
} catch (e) {
  console.log("alt threw", e instanceof TypeError, alt.length)
}

// the length written through an alias inside the loop
function aliased(q: number[], r: number[], n: number): number {
  let total = 0
  for (let i = 0; i < n; i = i + 1) {
    q.push(i)
    q.push(i * 2)
    if (i % 7 === 3) r.length = 1
    total = total + q.shift()!
  }
  return total + q.length * 1000
}
const a: number[] = []
console.log("aliased", aliased(a, a, 60), a.length)
