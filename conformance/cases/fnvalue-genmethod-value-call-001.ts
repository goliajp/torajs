// A generator method extracted as a value and called through an
// untyped parameter: the factory cell is Closure repr end-to-end.
function drive(fn) {
  const it = fn();
  console.log("first", it.next().value);
}
let o = {
  *f() {
    yield 7;
  },
};
drive(o.f);
const local = o.f;
drive(local);
