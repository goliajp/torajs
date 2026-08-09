// rotation 346 — Array.fromAsync's constructor-`this` face (RFC
// 20260808 B6 刀 4): a `.call(C, …)` receiver that is a constructor
// takes Construct(C) (iterable source, no arguments) or
// Construct(C, «len») (array-like), a CreateDataPropertyOrThrow per
// element, then length — instead of ignoring the receiver and
// answering a plain array.
function runTest(fn: any): void {
  fn();
}
runTest(async function () {
  let ctorCount = 0;
  function MyArray() {
    ctorCount++;
    this.tagged = true;
  }
  const a: any = await Array.fromAsync.call(MyArray, [10, 20]);
  console.log(a instanceof MyArray);
  console.log(Object.getPrototypeOf(a) === MyArray.prototype);
  console.log(a.length, a[0], a[1], a.tagged, ctorCount);

  const b: any = await Array.fromAsync.call(MyArray, { length: 2, 0: "x", 1: "y" });
  console.log(b instanceof MyArray, b.length, b[0], b[1], ctorCount);

  const plain: any = await Array.fromAsync.call(undefined, [7]);
  console.log(Array.isArray(plain), plain[0]);
});
