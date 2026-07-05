// inspect escape trunk chunk D — bun key-quote + string-escape rules:
// - object keys render bare only when they form an ASCII identifier
//   ([A-Za-z$_][A-Za-z0-9$_]*, bun isLatin1Identifier); everything
//   else (dashes, spaces) renders as a JSON-quoted string
// - quoted inspect strings (array elements / object values) go
//   through JSON escapes: \" \\ \n \t etc
// Integer-like keys ("0") are excluded here: bun orders them first
// (ES integer-key property order) while tr's dynobj iterates in
// insertion order — separate L3b trunk, not an inspect concern.
const o: any = {};
o["a-b"] = 1;
o["$x"] = 3;
o["_y"] = 4;
o["with space"] = 5;
console.log(o);
console.log(["a\"b", "a\\b", "a\nb", "a\tb"]);
console.log({ k: "v\"w" });
