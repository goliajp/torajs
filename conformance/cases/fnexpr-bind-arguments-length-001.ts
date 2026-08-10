var obj = { prop: "abc" };
var func = function () {
  console.log("this-ok:", this === obj);
  console.log("len:", arguments.length);
  return this === obj && arguments.length === 0;
};
var newFunc = Function.prototype.bind.call(func, obj);
console.log(newFunc());
