// a then-chain whose links each carry one reaction, mixed with links
// that carry two, string and void returns, a throwing handler and a
// recovery, checked against the interleaving of the chains
function add1(v: number): number { return v + 1 }
function twice(v: number): number { return v * 2 }

const log: string[] = []

let p = Promise.resolve(0)
for (let i = 0; i < 5; i = i + 1) {
  p = p.then(add1)
}
const fork = p.then(twice)
p.then((v: number) => { log.push('a' + v) })
fork.then((v: number) => { log.push('b' + v) })

const s = Promise.resolve(3)
  .then((v: number) => 'n' + v)
  .then((t: string) => t + '!')
  .then((t: string) => { log.push(t) })

function thrower(v: number): number {
  if (v > 0) throw new Error('boom')
  return v
}
function recover(e: any): number {
  log.push('caught ' + e.message)
  return 7
}
const q = Promise.resolve(1).then(thrower).then(add1).catch(recover).then(add1)
q.then((v: number) => { log.push('q' + v) })

let other = Promise.resolve(100)
for (let i = 0; i < 4; i = i + 1) {
  other = other.then((v: number) => { log.push('o' + v); return v + 1 })
}

// closure handlers that capture a local, a finally link, and a link that
// returns a promise beside links that return plain values
function closures(k: number): Promise<number> {
  let step = k
  let c = Promise.resolve(k)
  for (let i = 0; i < 3; i = i + 1) {
    c = c.then((v: number) => { step = step + 1; log.push('c' + v + ':' + step); return v + step })
  }
  return c
    .finally(() => { log.push('fin' + step) })
    .then((v: number) => Promise.resolve(v * 10))
    .then((v: number) => { log.push('adopt' + v); return v + 1 })
}
const cl = closures(2)

async function main(): Promise<void> {
  const y = await other
  await s
  log.push('y' + y)
  const w = await fork
  console.log(w, await cl)
  console.log(log.join(' '))
}
main()
