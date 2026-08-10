// §10.4.4.6 step 21 — `callee` is an OWN property of an arguments
// object (10.6-14-c-1-s: verifyProperty's existence probe rides
// hasOwnProperty, which used to miss the virtual accessor).
var argObj: any = (function () {
  return arguments;
})(1);
console.log(argObj.hasOwnProperty("callee"));
console.log(argObj.propertyIsEnumerable("callee"));
console.log(argObj.hasOwnProperty("length"), argObj.hasOwnProperty("0"));
