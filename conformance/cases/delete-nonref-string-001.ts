// delete on non-reference operands (13.5.1.2 step 2 true-lane, void-fold
// disambiguation) and the String-receiver exotic face (10.4.3:
// in-range index / length refuse under module-strict, absent keys true).
var a = { b: 42 };
console.log(delete void a.b);
console.log(delete void 0);
console.log(delete typeof 0);
console.log(delete delete 0);
console.log(delete {x:1});
console.log(delete 'Test262'[100]);
console.log(delete +-~!0);

// in-range string index delete must throw (strict, non-configurable)
try {
  var s: any = "abc";
  console.log(delete s[0]);
} catch (e: any) {
  console.log("caught:" + (e instanceof TypeError));
}
// length too
try {
  var s2: any = "abc";
  console.log(delete s2["length"]);
} catch (e: any) {
  console.log("caught2:" + (e instanceof TypeError));
}
// out of range: fine
var s3: any = "abc";
console.log(delete s3[99]);
console.log(delete "lit"[99]);
