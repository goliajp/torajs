// An object-literal accessor's own `arguments` inside an arrow, an
// async function and another accessor; an arrow inside the accessor
// sees the accessor's.

function inArrow(a: number, b: number) {
  const mk = () => ({ get g() { return arguments.length + ":" + typeof arguments; } });
  console.log("arrow", mk().g, arguments.length);
}
inArrow(1, 2);

async function inAsync(a: number, b: number) {
  await null;
  const o = { get g() { return arguments.length + ":" + typeof arguments; } };
  console.log("async", o.g);
}
inAsync(1, 2);

function nestedAccessor(a: number, b: number) {
  const o = {
    get g() {
      const p = { get h() { return arguments.length; } };
      return p.h + arguments.length;
    },
  };
  console.log("nested accessor", o.g);
}
nestedAccessor(1, 2);

function innerArrow(a: number, b: number) {
  const o = { get g() { const k = () => arguments.length; return k(); } };
  console.log("inner arrow", o.g);
}
innerArrow(1, 2);

function f1() {
  const o = { get g() { return arguments.length + 100; } };
  return o.g;
}
function f2() {
  const o = { get g() { return 5; } };
  return o.g;
}
console.log(f1(), f2());
