// Any-method-call RFC 20260704 C2 — String methods on any receivers:
// indexOf / includes / slice / split / trim family, heap-Str and
// ShortStr receiver shapes.
const s: any = "hello world";
console.log(s.indexOf("o"));
console.log(s.indexOf("o", 5));
console.log(s.indexOf("zz"));
console.log(s.includes("world"));
console.log(s.includes("xyz"));
console.log(s.slice(1, 4));
console.log(s.slice(-5));
console.log(s.slice(6));
const parts: any = s.split(" ");
console.log(parts.length);
console.log(parts[0]);
console.log(parts[1]);
const t: any = "  pad  ";
console.log(t.trim());
console.log(t.trimStart());
console.log(t.trimEnd());
const u: any = "abc";
console.log(u.indexOf("b"));
console.log(u.slice(1));
console.log(u.split("").length);
