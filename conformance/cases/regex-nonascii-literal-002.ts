// A pattern character past ASCII is one atom: quantifiers, lookbehind
// and alternation act on the whole character, and without `u` a
// character past the BMP is two code units. Acceptance: byte-equal
// with bun.

console.log(/é+/i.exec("xÉé"));
console.log(/(?<=日+)a/.exec("日日a"));
console.log(/(?<=😀+)a/u.exec("😀😀a"));
console.log(/(?<!é)a/.exec("éa a"));
console.log(/é|éa/.exec("éa"));
console.log(/日本|日/.exec("日本"));
console.log(/😀{2}/.test("😀😀"), /😀{2}/u.test("😀😀"));
console.log(/日|b/.exec("aéb"));
console.log(/[^日]+/.exec("aé"));
console.log(/^.$/.test("😀"), /^.$/u.test("😀"));
console.log(/[😀]/u.exec("😀"));
console.log("éé=22".match(/(é+)=/));
console.log(new RegExp("(é+)=").exec("éé=22"));
