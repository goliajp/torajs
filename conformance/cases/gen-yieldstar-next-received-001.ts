// A sync `yield*` delegation calls the inner iterator's `next` with
// exactly ONE argument (§27.5.3.2 step 7.a.i): undefined first, then
// whatever the outer generator was resumed with. The generic lane
// used to call `next()` with none and was right only through the
// name-keyed default padding of a machine's own `next(__yield_arg)`.
function Iter(this: any) {
  function next() { if (arguments.length != 1) throw Error("argc " + arguments.length); return { value: 42, done: true }; }
  this.next = next;
  this[Symbol.iterator] = function () { return this; };
}
function* delegate(iter: any): Generator<any> { return yield* iter; }
const it = delegate(new (Iter as any)());
console.log("first", JSON.stringify(it.next()));
const log: any[] = [];
const inner: any = { i: 0, [Symbol.iterator]() { return this; }, next(v: any) { log.push(v); this.i++; return { value: this.i, done: this.i > 2 }; } };
function* d2(): Generator<any> { const r = yield* inner; log.push("r=" + r); }
const g2 = d2(); g2.next("a"); g2.next("b"); g2.next("c");
console.log("received", JSON.stringify(log));
