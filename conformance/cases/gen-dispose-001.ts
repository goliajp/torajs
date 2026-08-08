// RFC 20260809 B6 — generator [@@dispose] through the real
// %Iterator.prototype% own entries (§27.1.2.1 [@@iterator]
// return-this + §27.1.4.1 [@@dispose]): a generator instance
// inherits both through its actual prototype chain (instance →
// class proto → %GeneratorPrototype% → %Iterator.prototype%), so
// dispose drives return() — finally blocks run — and `using` can
// hold a generator. An object hand-built on Iterator.prototype
// with no return() disposes as a quiet no-op.
function* g() {
  try {
    yield 1;
    yield 2;
  } finally {
    console.log("cleanup");
  }
}
const it: any = g();
console.log(typeof it[Symbol.dispose]);
console.log(typeof it[Symbol.iterator]);
const self: any = it[Symbol.iterator]();
console.log(self === it);
console.log(it.next().value);
console.log(it[Symbol.dispose]());
console.log(it.next().done);

function useGen(): void {
  using u: any = g();
  console.log(u.next().value);
}
useGen();

const fresh: any = g();
console.log(fresh[Symbol.dispose]());
console.log(fresh.next().done);

const plain: any = Object.create((Iterator as any).prototype);
console.log(typeof plain[Symbol.dispose]);
console.log(plain[Symbol.dispose]());
console.log("after");
