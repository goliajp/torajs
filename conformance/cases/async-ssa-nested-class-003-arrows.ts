// An async method of a class declared inside an arrow suspends over its
// frame wherever the arrow stands: a parameter's default, a static field
// initializer, a method parameter's default.

const log: string[] = [];
function f(g = () => { class D { async m() { await 0; log.push("D"); return 4 } } return new D().m() }) {
  return g();
}
class S {
  static h = () => { class E { async m() { await 0; log.push("E"); return 5 } } return new E().m() };
  k(p = () => { class F { async m() { await 0; log.push("F"); return 6 } } return new F().m() }) {
    return p();
  }
}
Promise.resolve().then(() => log.push("t0")).then(() => log.push("t1")).then(() => log.push("t2"));
Promise.all([f(), S.h(), new S().k()]).then((v) => {
  log.push(v.join("+"));
  console.log(log.join(" "));
});
