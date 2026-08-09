// B6 residuals (RFC 20260809-using-declarations): the sync dispose
// pair is one function object (spec pins prototype[@@dispose] to
// %DisposableStack.prototype.dispose%; the async pair stays distinct
// — bun parity), and both prototypes carry a W0/E0/C1 @@toStringTag.
// Both entries are own writes past the proto's 7-entry initial dense
// capacity — they exercise the store-split resize (address-stable
// header) landed in the same rotation.
console.log((DisposableStack.prototype as any)[Symbol.dispose] === (DisposableStack.prototype as any).dispose);
console.log((AsyncDisposableStack.prototype as any)[Symbol.asyncDispose] === (AsyncDisposableStack.prototype as any).disposeAsync);
const ds: any = new DisposableStack();
console.log(ds[Symbol.toStringTag]);
const ads: any = new AsyncDisposableStack();
console.log(ads[Symbol.toStringTag]);
const d: any = Object.getOwnPropertyDescriptor(DisposableStack.prototype, Symbol.toStringTag);
console.log(d.value, d.writable, d.enumerable, d.configurable);
const s2 = new DisposableStack();
let disposed = false;
s2.use({
  [Symbol.dispose]() {
    disposed = true;
  },
});
(s2 as any)[Symbol.dispose]();
console.log(disposed, s2.disposed);
