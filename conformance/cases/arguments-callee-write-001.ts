// §10.4.4.6 step 21 — callee's %ThrowTypeError% pair covers all
// three touch positions: a WRITE evaluates the RHS then throws
// (§13.15.2 PutValue on the accessor), a DELETE throws (§13.5.1.2
// step 3.a — strict, non-configurable), and the throw crosses the
// fn boundary (the thrower joins the may-throw analysis; the
// S10.6_A3_T4 shape used to strand the pending throw and SIGSEGV).
function w() {
  arguments.callee = "x";
}
try {
  w();
  console.log("no throw");
} catch (e) {
  console.log("write", e instanceof TypeError);
}
function del2() {
  return arguments.callee.caller;
}
try {
  del2();
  console.log("no throw");
} catch (e) {
  console.log("read", e instanceof TypeError);
}
