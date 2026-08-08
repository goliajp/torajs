var evalStr = '1' + '+' + '2';
var s2 = '5 * 2;';
var t = '"outer";';
var ind = '"global";';
function f() {
  var local = '7;';
  return eval(local);
}
console.log(eval(evalStr));
console.log(eval(s2));
console.log(f());
console.log(eval(t));
console.log((0, eval)(ind));
