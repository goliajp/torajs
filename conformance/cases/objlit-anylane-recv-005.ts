// RFC 20260717-objlit-anylane-recv knife 2d — the thisArg of
// `f.call` / `f.apply` / `f.bind` rides a recv-first closure's
// receiver channel. Pre-fix all three lanes dropped it, so the
// promoted body's __this ate the first user argument (call/apply
// answered NaN, bind bound the wrong this).

const o: any = { v: 7, f(a) { return this.v + a; } };
const other: any = { v: 100 };

// call with an explicit thisArg
console.log(o.f.call(other, 1)); // 101

// apply with an args list
console.log(o.f.apply(other, [2])); // 102

// detached then re-attached through call
const g = o.f;
console.log(g.call(o, 3)); // 10

// bind carries the bound this into every invocation
const bound = o.f.bind(other);
console.log(bound(4)); // 104
console.log(bound(5)); // 105

// bind with a partial arg: bound args land AFTER the receiver
const bound2 = o.f.bind(other, 9);
console.log(bound2()); // 109

// plain (this-free) closure through call still drops the thisArg
const plain: any = (x) => x * 2;
console.log(plain.call(other, 21)); // 42
console.log("done");
