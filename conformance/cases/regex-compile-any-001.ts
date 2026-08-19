// Annex B §B.2.4.1 RegExp.prototype.compile through an `any`
// receiver — in-place recompile: pattern/flags swap, lastIndex
// resets, and the answer IS the receiver (step 5 returns O).
var re: any = /ab/g;
console.log("" + re, re.test("xaby"), re.lastIndex);
var back: any = re.compile("cd", "i");
console.log("" + re, re.test("xCDy"), re.lastIndex, back === re);
re.compile(/ef/m);
console.log("" + re, re.test("ef"));
