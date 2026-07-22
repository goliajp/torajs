// arity-aware inference of <string>.replace(regex, cb): capture groups
// -> String, trailing (offset, input) -> (number, string), by the
// literal's static capture count (§22.1.3.19 GetSubstitution).
console.log("cap:", "a1b".replace(/([0-9])/, (m, c1) => `[${m}=${c1}]`));
console.log("offin:", "hello".replace(/l/, (m, offset, input) => `${m}@${offset}/${input}`));
console.log("mixed:", "x5y".replace(/(\d)/, (m, c1, offset, input) => `${c1}#${offset}#${input}`));
console.log("two-cap:", "abab".replaceAll(/(a)(b)/g, (m, p1, p2) => `${p1}-${p2}`));
const re = /o/g;
console.log("var-regex:", "foo".replace(re, (m) => m.toUpperCase()));
