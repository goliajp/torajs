// §15.1.2 / §15.8.1 formal-parameter legal faces: ordinary defaults,
// a template-interpolation default (sub-parser inherits the flag),
// and a nested arrow BODY inside a default (its statements are their
// own context) must keep working after yield/await are rejected
// inside formal parameter lists.
function f(x: any = 1 + 2): any {
  return x;
}
console.log(f());
console.log(f(9));

function g(s: any = `v${1 + 1}`): any {
  return s;
}
console.log(g());

function h(cb: any = () => 7): any {
  return cb();
}
console.log(h());

class C {
  m(y: any = 5): any {
    return y;
  }
}
console.log(new C().m());
