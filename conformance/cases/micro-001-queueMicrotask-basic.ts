// P10.1-A1 — queueMicrotask schedules cb on the microtask queue.
// Synchronous code completes first; queued cb fires when the
// queue drains (auto-drain at main exit via T-15.e).

queueMicrotask(() => console.log("mt1"))
console.log("sync")
