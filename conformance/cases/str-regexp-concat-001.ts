// §13.15.3 ToPrimitive on a RegExp operand of + reaches §22.2.6.14
// toString: "/source/flags". The new RegExp(...) spelling types
// Type::RegExp and used to refuse (literal bindings already worked).
console.log("got: " + (new RegExp("abc", "g")));
console.log((new RegExp("a")) + "!");
var q = new RegExp("x+", "i");
console.log("q: " + q + " end");
var lit = /xy+z/m;
console.log("lit: " + lit);
