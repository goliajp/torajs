// RFC 20260805 blade 1 — two async functions interleave one await-step
// at a time instead of each running to completion in turn.
async function a(): Promise<void> {
  console.log("a1");
  await Promise.resolve(0);
  console.log("a2");
  await Promise.resolve(0);
  console.log("a3");
}
async function b(): Promise<void> {
  console.log("b1");
  await Promise.resolve(0);
  console.log("b2");
  await Promise.resolve(0);
  console.log("b3");
}
a();
b();
console.log("sync-end");
