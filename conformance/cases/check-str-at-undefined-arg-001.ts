// S222 — String.{at,charAt,charCodeAt,codePointAt}(undefined) per ES
// §22.1.3.{1,2,3,4} step 2-3: ToIntegerOrInfinity(undefined)=0, so an
// explicit undef index behaves as index=0.
console.log("abc".at(undefined));
console.log("abc".charAt(undefined));
console.log("abc".charCodeAt(undefined));
console.log("abc".codePointAt(undefined));
console.log("hello".at(undefined));
console.log("hello".charAt(undefined));
console.log("hello".charCodeAt(undefined));
console.log("hello".codePointAt(undefined));
console.log("z".at(undefined));
console.log("z".charCodeAt(undefined));
