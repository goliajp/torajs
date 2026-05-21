// P10.2-A2 — Promise.{race,any}(ps).then(cb) lowering.
// Builds on A1/A1.1. Source-callee shape recognition in
// ssa_lower's static_ctor whitelist now includes the full
// Promise namespace static set (resolve/reject/all/race/any/
// allSettled), so chained .then/.catch/.finally on these
// results lowers through promise_then_* helpers instead of
// bouncing off the user-class fallback (which reported
// "unsupported member call shape: then").
//
// race/any on Promise<Undefined> array yields Promise<Undefined>
// per the check.rs:5343 default arm; A1.1's .then on
// Promise<Undefined> with 0-arg cb takes over from there.

const ps = [Promise.resolve(), Promise.resolve()]
Promise.race(ps).then(() => console.log("race-done"))
Promise.any(ps).then(() => console.log("any-done"))
console.log("sync")
