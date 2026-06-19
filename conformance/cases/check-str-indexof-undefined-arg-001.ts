// S214 — String.prototype.indexOf(needle, undefined) per ES §22.1.3.8 step 4:
// ToIntegerOrInfinity(undefined)=0, so fromIndex=undefined behaves as 0.
console.log("abcabc".indexOf("a", undefined));
console.log("abcabc".indexOf("b", undefined));
console.log("abcabc".indexOf("c", undefined));
console.log("abcabc".indexOf("x", undefined));
console.log("hello".indexOf("ll", undefined));
console.log("".indexOf("x", undefined));
console.log("abcabc".indexOf("", undefined));
console.log("abcabc".indexOf("abc", undefined));
console.log("aaaa".indexOf("aa", undefined));
