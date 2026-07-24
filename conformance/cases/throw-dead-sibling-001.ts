// Dead statements after a terminating statement must not execute —
// the fn-body / top-level stmt walks stop at a closed block, same as
// the Block / try-body / switch-case walks (S12.1_A5 shape).
function afterReturn(): number {
  return 1;
  console.log("dead-after-return");
}
function afterThrow(): void {
  throw new Error("inner");
  console.log("dead-after-throw");
}
console.log(afterReturn());
try {
  afterThrow();
} catch (e) {
  console.log("caught");
}
function mk(x: number): number {
  return x + 1;
}
try {
  throw 1;
  throw mk(2);
  console.log("dead-in-try");
} catch (e) {
  console.log("caught2", e);
}
console.log("done");
