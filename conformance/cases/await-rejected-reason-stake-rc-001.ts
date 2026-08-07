// Rotation 326 — awaiting a rejected promise handed the throw slot a
// BORROW of the rejection reason: the throw contract is owned (a
// thrown `new Error` transfers its mint; the catch binding releases
// it), but the promise cell keeps holding the reason and releases it
// again at its own drop — one reference, two charges, and the Error
// instance underflowed. A string reason survived only because static
// cells no-op rc. get_value's rejected arm now funds the slot's copy.
async function caught() {
  const p: any = Promise.reject(new Error("boom"));
  try {
    await p;
  } catch (e: any) {
    console.log("caught", e.message);
  }
}
caught();

// the same reason surviving TWO awaits — each throw is funded
async function twice() {
  const p: any = Promise.reject(new Error("again"));
  try {
    await p;
  } catch (e: any) {
    console.log("first", e.message);
  }
  try {
    await p;
  } catch (e: any) {
    console.log("second", e.message);
  }
}
twice();
