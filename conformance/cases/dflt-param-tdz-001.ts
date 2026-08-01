// rotation 275 刀 3 — §8.6.2 default-parameter TDZ: a default that
// bare-reads its own parameter or a later sibling throws a
// ReferenceError at call time (params environment, not the outer
// scope), across every function form. Supplying the argument skips
// the initializer entirely.

var x = 0;
var y = 9;

function probe(fn: () => any): void {
  try {
    fn();
    console.log("no throw");
  } catch (e) {
    console.log((e as Error).name);
  }
}

// ref-self, plain fn decl
function fa(x = x) {
  return 1;
}
probe(() => fa());

// ref-later, plain fn decl
function fb(a = y2, y2 = 1) {
  return a;
}
probe(() => fb());

// fn expression
var fc = function (x = x) {
  return 1;
};
probe(() => fc());

// arrow
var fd = (x = x) => 1;
probe(() => fd());

// class method
class C {
  m(x = x) {
    return 1;
  }
}
probe(() => new C().m());

// generator (throws on the factory call, body never runs)
function* fg(x = x) {
  yield 1;
}
probe(() => fg());

// async generator factory
var callCount = 0;
var fh = async function* (x = x) {
  callCount = callCount + 1;
};
probe(() => fh());
console.log(callCount);

// supplying the argument skips the initializer
function fi(x = x) {
  return x;
}
console.log(fi(5));
function fj(a = y2, y2 = 1) {
  return a;
}
console.log(fj(7));

// earlier-param reference stays legal
function fk(a: any, b = a) {
  return b;
}
console.log(fk(3));
