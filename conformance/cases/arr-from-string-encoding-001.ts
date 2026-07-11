// RFC 20260711 follow-up — Array.from(string) iterates code points
// (ES §23.1.2.1 string-iterator semantics: surrogate pairs group).
// The pre-fix kernel emitted one element per payload byte, garbling
// UTF-16 sources.
console.log(Array.from("abc"));
console.log(Array.from("汉字"));
console.log(Array.from("𝄞a𝄢"));
console.log(Array.from("café"));
console.log(Array.from(""));
const els = Array.from("汉a字");
console.log(els.join("|"));
console.log(els.length, els[0], els[1], els[2]);
