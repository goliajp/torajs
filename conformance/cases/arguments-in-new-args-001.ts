var func = function () {
  return new Boolean(arguments.length === 0);
};
console.log(func().valueOf());
console.log(func(1, 2).valueOf());
var NewFunc = Function.prototype.bind.call(func);
var newInstance = new NewFunc();
console.log(newInstance.valueOf());
var g = function () {
  var arr = new Array(arguments.length);
  return arr.length;
};
console.log(g(1, 2, 3));
var h = function () {
  return new Boolean(arguments[0]);
};
console.log(h(true).valueOf());
