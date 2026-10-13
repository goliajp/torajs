// §10.2.11 FunctionDeclarationInstantiation in a generator or async
// body: a `function z` and a `var z` of the body are one binding,
// initialized to the function before the first statement runs, then
// overwritten where an initializer or an assignment runs. The same
// steps as an ordinary body (`fn-body-var-over-fn-001.ts`); these
// bodies are rewritten into state machines first.

function* gen() {
  var z;
  function z() { return 1; }
  yield typeof z;
  yield z();
  z = 3;
  yield z;
}
console.log("gen", [...gen()].join(","));

function* genInit() {
  yield typeof z;
  var z = 4;
  function z() { return 1; }
  yield z;
}
console.log("gen-init", [...genInit()].join(","));

// a declaration reading the generator's own locals
function* genCapture(k: number) {
  var z;
  function z() { return k; }
  yield z();
  z = k + 1;
  yield z;
}
console.log("gen-capture", [...genCapture(5)].join(","));

async function asyncInit() {
  var z = 5;
  function z() {}
  return z;
}

async function asyncBefore() {
  var a = typeof z;
  await null;
  var z = 6;
  function z() { return 1; }
  return a + ":" + z;
}

async function asyncAssign() {
  var z;
  function z() { return 2; }
  var first = z();
  await null;
  z = "s";
  return first + ":" + z;
}

async function main() {
  console.log("async-init", await asyncInit());
  console.log("async-before", await asyncBefore());
  console.log("async-assign", await asyncAssign());
}
main();

// a declaration writing the shared binding from inside
function* genInner() {
  var z;
  function z() { return 1; }
  function set() { z = 8; }
  yield typeof z;
  set();
  yield z;
}
console.log("gen-inner", [...genInner()].join(","));
