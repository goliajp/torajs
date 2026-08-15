function f() {
  "use strict";
  try {
    delete Object.prototype;
    return "no-throw";
  } catch (e) {
    return "throw " + e.constructor.name;
  }
}
console.log(f());
function g() {
  return delete Object.prototype;
}
console.log(g());
const arr = [1, 2, 3];
function h() {
  "use strict";
  try {
    delete arr.length;
    return "no-throw";
  } catch (e) {
    return "throw " + e.constructor.name;
  }
}
console.log(h());
