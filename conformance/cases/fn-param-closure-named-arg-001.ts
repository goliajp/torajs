// A named function handed to a closure-typed parameter through a
// member call: a let-bound arrow stored in an object literal, and a
// class method whose parameter also receives an arrow. The function's
// value there is the same object every read of its name yields.
function named(v: number) {
  console.log("named", v);
}
let t = (f: (v: number) => void) => f(1);
const o = { k: t };
o.k(named);
o.k((v: number) => console.log("arrow", v));
class R {
  run(f: (v: number) => void) {
    f(5);
  }
  same(f: (v: number) => void) {
    return f === named;
  }
}
const r = new R();
r.run((v: number) => console.log("r arrow", v));
r.run(named);
r.same((v: number) => {});
console.log(r.same(named));
const fs: ((v: number) => void)[] = [];
fs.push(named);
fs.push((v: number) => console.log("pushed", v));
for (const f of fs) o.k(f);
