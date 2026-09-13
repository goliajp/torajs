// A throw while a yield's operand is being evaluated is an abrupt
// completion of the body (§27.5.3.7 step 3 evaluates the operand
// before the generator suspends): the generator is COMPLETED, and the
// next `next()` answers `{ value: undefined, done: true }` without
// running the statements after the yield. The machine used to persist
// the resume label before evaluating the operand, so it resumed at
// the post-yield arm.
function boom(): any { throw "boom"; }
function boomNum(): number { throw "boom-num"; }

function* syncAny() { yield boom(); console.log("sync-any resumed"); yield 2; }
function* syncTyped(): Generator<number> { yield boomNum(); console.log("sync-typed resumed"); yield 2; }
function* syncInto() { const x: any = yield boom(); console.log("sync-into resumed", x.foo); }
async function* asyncAny() { yield boom(); console.log("async-any resumed"); yield 2; }
const src = { [Symbol.asyncIterator]() { return { next() { return { get done() { throw "done-getter"; } }; } }; } };
async function* asyncStar() { yield* src; console.log("async-star resumed"); }

function drive(name: string, it: any) {
  try { it.next(); console.log(name, "did not throw"); } catch (e: any) { console.log(name, "threw", e); }
  const r = it.next();
  console.log(name, "then", r.done, r.value);
}
drive("sync-any", syncAny());
drive("sync-typed", syncTyped());
drive("sync-into", syncInto());

async function driveAsync(name: string, it: any) {
  try { await it.next(); console.log(name, "did not reject"); } catch (e: any) { console.log(name, "rejected", e); }
  const r: any = await it.next();
  console.log(name, "then", r.done, r.value);
}
async function main() {
  await driveAsync("async-any", asyncAny());
  await driveAsync("async-star", asyncStar());
}
main();
