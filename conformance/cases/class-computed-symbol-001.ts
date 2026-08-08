// RFC 20260809 刀 4 — class computed Symbol.<x> member keys define
// under the real Symbol cell (the legacy __sym_ name fold now serves
// Symbol.iterator alone): instance read/call, static read, using
// integration, and the iterator protocol's name-fold path staying
// intact. (@@toPrimitive DISPATCH from `+obj` is a separate gap —
// the numeric-coercion kernel does not consult the hook yet.)
const log: string[] = [];
class R {
  [Symbol.dispose]() { log.push("class-d"); }
}
{
  using r = new R() as any;
  log.push("body");
}
console.log(log.join(","));

class C {
  [Symbol.dispose]() { return 7; }
  named(): number { return 5; }
  static [Symbol.asyncDispose]() { return 22; }
}
const c: any = new C();
console.log(typeof c[Symbol.dispose], c[Symbol.dispose](), c.named());
console.log(typeof (C as any)[Symbol.asyncDispose]);

class T2 {
  [Symbol.toPrimitive](hint: string) { return 42; }
}
const t2: any = new T2();
console.log(typeof t2[Symbol.toPrimitive], t2[Symbol.toPrimitive]("number"));

class It {
  n: number = 0;
  [Symbol.iterator]() { return this; }
  next(): { value: number; done: boolean } {
    this.n = this.n + 1;
    return this.n <= 2 ? { value: this.n, done: false } : { value: 0, done: true };
  }
}
const it = new It();
const out: number[] = [];
for (const v of it) { out.push(v); }
console.log(out.join(","));
