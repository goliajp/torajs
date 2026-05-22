// P10.4-A2 — `await rejectedPromise` throws the rejection reason as a
// catchable exception per ES spec §27.2.5.2 / §27.2.5.7. Pre-fix tora's
// __torajs_promise_get_value silently returned 0 for REJECTED state
// (explicit "placeholder" comment in the runtime), so awaiting a
// rejected promise looked like a successful await of 0 — every
// try/catch around an awaited rejection silently fell through.

// Path A — primitive Number rejection caught
async function failNum(): Promise<number> {
  let p: Promise<number> = Promise.reject(99)
  return await p
}
try {
  let v = await failNum()
  console.log('no-throw-A', v)
} catch (e) {
  console.log('caught-num')
}

// Path B — caller direct await on rejected
let p2: Promise<number> = Promise.reject(7)
try {
  let v = await p2
  console.log('no-throw-B', v)
} catch (e) {
  console.log('caught-direct')
}

// Path C — regression guard: await fulfilled still returns normally
let p3: Promise<number> = Promise.resolve(123)
let v3 = await p3
console.log('fulfilled', v3)

// Path D — Promise.reject() 0-arg (P10.2-A1 form) reason = undefined
async function failUndef(): Promise<number> {
  let p: Promise<number> = Promise.reject() as any
  return await p
}
try {
  let v = await failUndef()
  console.log('no-throw-D', v)
} catch (e) {
  console.log('caught-undef')
}
