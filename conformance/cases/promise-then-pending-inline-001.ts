// `then` / `catch` with a plain handler on a source that is still
// pending: several reactions on one source fire in registration order
// (§27.2.1.4 TriggerPromiseReactions), each in its own job, interleaved
// with jobs queued around them; a settled source queues at once.

const log: string[] = []

function a(v: number): number {
  log.push("a")
  return v + 1
}

function b(v: number): number {
  log.push("b")
  return v + 10
}

function up(s: string): string {
  log.push("up")
  return s + "!"
}

function onErr(e: string): string {
  log.push("err")
  return "caught " + e
}

// pending sources: each `then` result below is pending when the next
// reaction is attached to it
const p = Promise.resolve(1).then(a)
const p1 = p.then(a)
const p2 = p.then(b)
const p3 = p1.then(b)
queueMicrotask(() => log.push("job"))

// a settled source: the reaction is queued now
const s = Promise.resolve(5).then(b)

// a chain built while every link is pending
let chain = Promise.resolve(100)
for (let i = 0; i < 4; i = i + 1) {
  chain = chain.then(a)
}

// string-valued: a rejection forwarded through a pending link, caught
const q = Promise.reject("no").then(up)
const q1 = q.catch(onErr)
const q2 = q.then(up).catch(onErr)
const q3 = Promise.resolve("yes").then(up).catch(onErr).then(up)
queueMicrotask(() => log.push("job2"))

console.log(await p1, await p2, await p3, await s)
console.log(await chain)
console.log(await q1, await q2, await q3)
console.log(log.join(","))
