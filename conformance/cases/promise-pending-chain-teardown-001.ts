// A million-link then chain parked on a promise nobody can settle.
// Only the link before holds each link (its list, or on the `any`
// lane the block of the record in its list), so when the head goes
// the whole chain is torn down from it: one link after another, not
// a nested call per link, which would run out of stack long before
// the end.

function never(): Promise<number> {
  return new Promise<number>(() => {})
}

function add1(x: number): number {
  return x + 1
}

function plain(n: number): number {
  const h = never()
  let p = h
  for (let i = 0; i < n; i++) {
    p = p.then(add1)
  }
  return n
}

function closing(n: number, k: number): number {
  const h = never()
  let p = h
  for (let i = 0; i < n; i++) {
    p = p.then((x: number) => x + k)
  }
  return n + k
}

function anyLane(n: number): number {
  const h: any = never()
  let p: any = h
  for (let i = 0; i < n; i++) {
    p = p.then((x: any) => x)
  }
  return n
}

// The head outlives the function that built the chain and is
// released later, by overwriting the only slot holding it.
const box: { p: Promise<number> } = { p: never() }

function hang(n: number): number {
  let p = box.p
  for (let i = 0; i < n; i++) {
    p = p.then(add1)
  }
  return n
}

console.log(plain(1000000))
console.log(closing(1000000, 1))
console.log(anyLane(1000000))
console.log(hang(1000000))
box.p = never()
console.log("done")
