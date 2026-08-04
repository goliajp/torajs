// RFC 20260805 blade 1 — `await` suspends: the caller gets control back
// at the first one, so the statement after the call runs BEFORE the
// body's continuation.
async function a(): Promise<void> {
  console.log("a-enter");
  await Promise.resolve(1);
  console.log("a-after-await");
}
console.log("before");
a();
console.log("after");
