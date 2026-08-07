// index-assign through an `any` key on a typed-array receiver rides
// the keyed set kernel (write mirror of the read-side Array arm).
// S12.6.3_A5 / using-syntax shape: a `var` binding reads as Any.
var using = [], x = 0;
{
  using[x] = null;
}
console.log(using.length, using[0]);

var a = [1, 2, 3];
var i = 1;
a[i] = 9;
console.log(a[0], a[1], a[2]);

var s: any = [4, 5];
var k: any = 0;
s[k] = 7;
console.log(s[0], s[1]);
