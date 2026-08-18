// Cluster #4 — a struct-typed key on an object receiver rides
// §7.1.19 ToPropertyKey through its own toString (the t262
// target-member-computed-reference shape).
var base = {};
var prop = { toString: function(): string { return "pk"; } };
base[prop] = 42;
console.log(base["pk"]);
console.log(base[prop]);

// Compound assignment: read-modify-write through the coerced key.
var base2: any = {};
base2["k2"] = 10;
var prop2 = { toString: function(): string { return "k2"; } };
base2[prop2] *= 3;
console.log(base2["k2"]);

// Evaluation order (S11.13.1_A7_T4 shape): ToPropertyKey happens in
// GetValue/PutValue — for plain assignment, after the rhs evaluates.
function mk(): any {
  var log: string[] = [];
  var b: any = {};
  var p = { toString: function(): string { log.push("key"); return "x"; } };
  var e = function(): number { log.push("rhs"); return 1; };
  b[p] = e();
  return log.join(",");
}
console.log(mk());

// A throwing toString surfaces as a catchable error before the store.
var caught = "";
try {
  var badp = { toString: function(): string { throw new Error("boom"); } };
  var b3: any = {};
  b3[badp] = 1;
} catch (err) {
  var e2: any = err;
  caught = e2.message;
}
console.log(caught);

// T4 shape — compound assignment coerces the key exactly once
// (§6.2.5: GetValue writes ToPropertyKey's answer back into the
// Reference Record; the embedded read and the store share it).
var coerces = 0;
var b4: any = {};
b4["once"] = 5;
var p4 = { toString: function(): string { coerces++; return "once"; } };
b4[p4] += 2;
console.log(b4["once"], coerces);
