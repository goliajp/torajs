// r499 — main's end-of-program drains on demand. The microtask
// drain is kept iff the microtask member has live text once the
// drain's own edge is ignored: queueMicrotask feeds it directly,
// with no Promise in the program (so the unhandled-rejection sweep
// is elided — exit code is the constant 0 — while the drain stays).
// A wrong elision would drop every queued callback's output.
let log: string[] = [];
queueMicrotask(() => {
  log.push("m1");
  queueMicrotask(() => log.push("m3"));
});
queueMicrotask(() => log.push("m2"));
log.push("sync");
console.log(log.join(","));
queueMicrotask(() => console.log(log.join(",")));
