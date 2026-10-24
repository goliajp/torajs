// Top-level awaits inside `try`, `catch` and `finally` blocks run their
// continuation one job after the awaited value settles, as anywhere in
// the module body (§27.7.5.3), interleaved with a `.then` chain. A
// rejection awaited in a `try` enters its `catch` with the reason; a
// `finally` that awaits still runs before the code after the statement,
// and a `finally` without an abrupt exit keeps the completion of its
// `try`.
const log: string[] = [];
let tick = 0;
function chain(n: number): void {
  Promise.resolve(0).then(function step(): void {
    log.push("tick " + ++tick);
    if (tick < n) Promise.resolve(0).then(step);
  });
}
chain(12);

try {
  const a: number = await Promise.resolve(1);
  log.push("try a=" + a);
  await Promise.reject(new Error("boom"));
  log.push("not reached");
} catch (e) {
  log.push("catch " + (e as Error).message);
  const b: number = await Promise.resolve(2);
  log.push("catch b=" + b);
} finally {
  log.push("finally start");
  await null;
  log.push("finally end");
}

let s = "";
for (let i = 0; i < 3; i++) {
  try {
    s += await Promise.resolve(String(i));
    if (i === 1) throw new Error("at " + i);
  } catch (e) {
    s += "!" + (e as Error).message;
  } finally {
    s += await ".";
  }
}
log.push("loop " + s);

try {
  try {
    await Promise.reject("inner");
  } finally {
    log.push("inner finally");
    await 0;
  }
} catch (e) {
  log.push("outer caught " + e);
}
for (const line of log) console.log(line);
