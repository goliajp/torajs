const x: any = 1;
console.log([5,6,5].indexOf(5, x, 99), [5,6,5].lastIndexOf(5, x, 99), [5,6,5].includes(5, x, 99));
console.log("abcdef".slice(0, "2"), "abcdef".slice("1", 4), "abcdef".slice(0, true), "abcdef".slice(null, 3));
console.log("abcdef".substring(1, "3"), "abcdef".substring("2", "5"));
console.log("abcdef".substr("1", "2"), "abcdef".substr(0, undefined), "abcdef".substr(1, undefined), "abcdef".substr(true, 3));
console.log("abc".padStart("5"), "abc".padStart(true), "abc".padStart("6", "*"), "abc".padEnd("5", "-"));
console.log("abcdef".slice("1", "3", 99), "abc".padStart("6", "*", 99));
console.log("abcdef".slice(0, undefined), "abcdef".substring(undefined, 3), "abcdef".slice(0, NaN), "abcdef".slice(2), "abcdef".substr(2));
console.log("abcdef".slice("x", 3), "abcdef".substr(0, "z"), "abc".padStart("bad"));
