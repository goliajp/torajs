// Every await here suspends. `await null` resolves null to an already
// fulfilled promise and attaches a reaction to it; a reaction on a settled
// promise is queued as a job, never run inline (Await, PerformPromiseThen),
// so step yields once to the microtask queue. While step is suspended its
// result promise is still pending, so the loop's await in turn suspends
// until step's resumption settles it: two microtask round trips per call.
async function step(i: number): Promise<number> {
  await null
  return i * 3 + 1
}

const n = parseInt(process.argv[2], 10)
let total = 0
for (let i = 0; i < n; i = i + 1) {
  total = total + (await step(i))
}
console.log(total)
