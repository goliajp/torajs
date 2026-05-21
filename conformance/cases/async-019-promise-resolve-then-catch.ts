// P10.2-A1.1 — .then / .catch on Promise<Undefined>.
// Builds on A1's 0-arg Promise.resolve() / .reject() ctors:
// chains a 0-arg cb `() => void` per the call-time arm. Both
// the resolved-then and rejected-catch paths fire as microtasks
// after the sync log (drain ordering proof, same shape as A1's
// async-018 but exercising .then/.catch helpers instead of
// .finally).

Promise.resolve().then(() => console.log("r1"))
Promise.reject().catch(() => console.log("r2"))
console.log("sync")
