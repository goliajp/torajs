// RFC 20260810-sloppy-goal-arguments S3 — mapped aliasing both ways
// on the simple-param static face (§10.4.4 CreateMappedArguments).
function w1(a: any, b: any, c: any) {
  a = 1;
  b = "str";
  c = 2.1;
  console.log("m1", arguments[0], arguments[1], arguments[2]);
}
w1(10, "sss", 1);

function w2(a: any, b: any, c: any) {
  arguments[0] = 1;
  arguments[1] = "str";
  arguments[2] = 2.1;
  console.log("m2", a, b, c);
}
w2(10, "sss", 1);

function w3(a: any, b: any) {
  a = 7;
  console.log("m3", arguments.length, arguments[0]);
}
w3(5, 6);
