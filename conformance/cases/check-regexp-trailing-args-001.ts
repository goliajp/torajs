// S266 — RegExp.{test,exec,toString}(s?, ...trailing) per ES
// §22.2.6.{2,7,16}. Trailing args silent-drop per ES trailing-
// arg ignore. tora's check.rs fixed-sig (`vec![Type::String]`
// / `Vec::new()`) rejected 1+ args; SSA-emit uses only args[0]
// (test/exec) or no args (toString); S266 widens both layers.

const re = /ab/;
console.log(re.test("abc"));
console.log(re.test("abc", 99));
console.log(re.test("xyz", 77, 66));
console.log(re.test("ab", 99, 88, 77));

const re2 = /a/;
console.log(re2.exec("abc").length);
console.log(re2.exec("abc", 99).length);
console.log(re2.exec("ab", 77, 66).length);

const re3 = /abc/g;
console.log(re3.toString());
console.log(re3.toString(99));
console.log(re3.toString(77, 66, 55));
