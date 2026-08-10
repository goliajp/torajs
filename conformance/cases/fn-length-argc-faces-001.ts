// fn `.length` compile-time fold across the argc-era faces (RFC
// 20260810-indirect-argc-abi S3.7): only user params count, and only
// up to the first default / rest (ES §10.2.5 ExpectedArgumentCount).
function plain(a: any, b: any) {
  return a;
}
function withDflt(a: any, b: number = 5) {
  return b;
}
function withRest(a: any, ...rest: any[]) {
  return rest.length;
}
function lenTop(a: any) {
  return arguments.length;
}
console.log(plain.length);
console.log(withDflt.length);
console.log(withRest.length);
console.log(lenTop.length);
console.log(lenTop(1, 2, 3));
const anonBind = function (x: any, y: any, z: any) {
  return x;
};
console.log(anonBind.length);
