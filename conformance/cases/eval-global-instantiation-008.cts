// A function that reads a name only a later global eval binds throws
// a ReferenceError its caller can catch, and once the eval ran the
// same function reads the binding.
function readVar() {
  return x;
}
function readFn() {
  return f1;
}
function callFn() {
  return f1(1);
}
for (const fn of [readVar, readFn, callFn]) {
  try {
    fn();
    console.log("no throw");
  } catch (e) {
    console.log(e.constructor.name);
  }
}
eval("var x = 1; function f1(v){ return v + 1 }");
console.log(readVar(), typeof readFn(), callFn());
