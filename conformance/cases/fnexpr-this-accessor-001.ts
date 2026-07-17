const o: any = {};
o.__defineSetter__("y", function (v: any) {
  this._y = v * 2;
});
o.__defineGetter__("g", function () {
  return this._y + 1;
});
o.y = 21;
console.log(o._y);
console.log(o.g);
o.__defineGetter__("k", function () {
  return 7;
});
console.log(o.k);
