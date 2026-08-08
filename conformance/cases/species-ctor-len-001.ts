// §23.1.3 ArraySpeciesCreate length argument, per family method:
// filter / concat seed an EMPTY product (0), map passes the source
// len, slice passes the clamped count, splice the actualDeleteCount.
// The pre-fix shortcut handed every method the receiver's length
// (filter's create-species.js asserts the ctor sees 0).
var lens: any = [];
function make(): any {
  var o: any = [1, 2, 3, 4, 5];
  o.constructor = {};
  o.constructor[Symbol.species] = function (n: any) {
    lens.push(n);
  };
  return o;
}
make().filter(function () {});
make().map(function (x: any) {
  return x;
});
make().slice(1, -1);
make().splice(2);
make().concat([9]);
console.log(lens.join(","));
