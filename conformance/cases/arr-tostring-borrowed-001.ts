// RFC 20260721 刀 11 G12 — the reified `Array.prototype.toString`
// carries its own id (§23.1.3.36): borrowed onto a receiver without
// a callable `join` it answers the %Object.prototype.toString%
// badge; a dynobj receiver's own callable `join` is invoked; an
// Array receiver keeps the ordinary join route.
console.log(Array.prototype.toString.call(true));
console.log(Array.prototype.toString.call(5));
console.log(Array.prototype.toString.call("xy"));
console.log(Array.prototype.toString.call([7, 8]));
let o: any = {
  join: function () {
    return "userjoin";
  },
};
console.log(Array.prototype.toString.call(o));
let f: any = Array.prototype.toString;
console.log(f.call(false));
console.log([1, [2, 3]].toString());
