// The named-group duplicate rule (§22.2.1.1 static semantics) walks
// every capture, but the parser's name table only grows as far as the
// LAST named group — a trailing unnamed group had no entry, and the
// lookup was an index-out-of-bounds at compile time. Unnamed groups
// also compared equal to each other as duplicates (two empty names).
// test262 named-groups/non-unicode-match, rotation 651.
console.log(JSON.stringify("baba".match(/(.)(?<a>a)(?<b>\1)(\2)/)));
console.log(JSON.stringify("abcd".match(/(?<a>a)(?<b>b)(.)(.)/)));
console.log(JSON.stringify("abcd".match(/(.)(?<a>b)(.)(?<b>d)/)));
console.log(JSON.stringify("abccba".match(/(?<a>.)(?<b>.)(?<c>.)\k<c>\k<b>\k<a>/)));
console.log(JSON.stringify("xy".match(/(?<a>x)|(?<a>y)/)));
console.log(JSON.stringify("baba".match(/(.)(?<a>a)(?<b>\1)(\2)/).groups));
