// §22.2.6.2 / §22.2.6.16 step 3: `S = ToString(string)` — an absent
// argument is `undefined`, so the haystack is the string "undefined".
// The typed lowering indexed `args[0]` outright and the compiler died
// on the empty argument list (test262 S15.10.6.2_A1_T16 / A12,
// rotation 651).
var r: any = /undefined/.exec();
console.log(r[0], r.index);
console.log(/x/.test(), /fine/.test(), /^undefined$/.test());
var m: any = new RegExp("(.|\r|\n)*", "").exec();
console.log(m[0]);
console.log(JSON.stringify(/(?:u)(n)/.exec()));
