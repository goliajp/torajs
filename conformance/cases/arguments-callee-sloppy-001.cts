// RFC 20260810-sloppy-goal-arguments S2 — sloppy callee faces:
// direct-spelling read, escaped keyed read + gOPD, for-in filter,
// delete (configurable), write (writable), delete-then-read.
function named(a: any) {
  console.log("r1", typeof arguments.callee);
}
named(1);

function esc(a: any, b: any) {
  return arguments;
}
var argObj: any = esc(2, 3);
console.log("r2", typeof argObj.callee);
var d: any = Object.getOwnPropertyDescriptor(argObj, "callee");
console.log("r3", d.writable, d.enumerable, d.configurable);
var seen: boolean = false;
for (var k in argObj) {
  if (k === "callee") seen = true;
}
console.log("r4", seen);

function delform() {
  return delete arguments.callee;
}
console.log("r5", delform());

function assignform() {
  arguments.callee = 42;
  return arguments.callee;
}
console.log("r6", assignform());

function delread() {
  delete arguments.callee;
  return typeof arguments.callee;
}
console.log("r7", delread());
