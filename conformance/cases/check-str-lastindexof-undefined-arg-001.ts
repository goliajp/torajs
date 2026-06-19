// S216 — String.prototype.lastIndexOf(needle, undefined) per ES
// §22.1.3.10 step 5: `ToNumber(undefined)=NaN, NaN→pos=+∞`. Effective
// fromIndex=length; search starts from the end.
console.log("abcabc".lastIndexOf("a", undefined));
console.log("abcabc".lastIndexOf("b", undefined));
console.log("abcabc".lastIndexOf("c", undefined));
console.log("abcabc".lastIndexOf("x", undefined));
console.log("hello".lastIndexOf("ll", undefined));
console.log("".lastIndexOf("x", undefined));
console.log("abcabc".lastIndexOf("", undefined));
console.log("abcabc".lastIndexOf("abc", undefined));
console.log("aaaa".lastIndexOf("aa", undefined));
