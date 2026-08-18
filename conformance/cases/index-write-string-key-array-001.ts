// cluster #3 (rotation 442): String / Symbol keys on a typed ARRAY
// receiver's keyed WRITE box and ride the keyed set kernel, whose
// §7.1.19 spelling routes canonical array-index strings to the
// element store — S15.4_A1.1_T4's `x["0"] = 0`.
var x: number[] = [];
x["0"] = 0;
console.log(x[0], x.length);
var y = [1, 2, 3];
y["1"] = 9;
console.log(y[1]);

// dynamic string key, canonical spelling
var k: string = "2";
y[k] = 30;
console.log(y[2]);

// non-canonical literal stores by property, elements untouched
y["foo"] = 7;
console.log((y as any).foo, y.length);

// a symbol key stores its own cell, uncoerced
var s: any = Symbol("t");
y[s] = 11;
console.log(y[s]);
