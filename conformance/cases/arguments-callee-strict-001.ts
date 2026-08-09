// §10.4.4.6 step 21 — on tr's always-strict module goal `callee` is
// the %ThrowTypeError% accessor: a direct read throws (the desugar
// rewrites the spelling to the runtime thrower), and the escaped
// gOPD answers an accessor descriptor — get/set present, no
// value/writable, enumerable and configurable both false (the
// 10.6-13-c strict family).
function direct() {
  try {
    arguments.callee;
    console.log("no throw");
  } catch (e) {
    console.log(e instanceof TypeError);
  }
}
direct(1);
function escaped() {
  var desc: any = Object.getOwnPropertyDescriptor(arguments, "callee");
  console.log(desc !== undefined);
  console.log(desc.configurable, desc.enumerable);
  console.log(desc.hasOwnProperty("value"), desc.hasOwnProperty("writable"));
  console.log(desc.hasOwnProperty("get"), desc.hasOwnProperty("set"));
  console.log(typeof desc.get, desc.get === desc.set);
}
escaped(2, 3);
