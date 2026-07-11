// String family any-dispatch slice 2 — padStart / padEnd / repeat /
// concat / codePointAt / localeCompare (mids 119-123 + the Str
// concat arm on mid 84).
const s: any = "abc";
const w: any = "aπ𝄞z"; // wide: BMP-nonlatin + astral (surrogate pair)
const sub: any = "xxhelloxx".slice(2, 7); // Substr receiver

console.log(s.padStart(6));
console.log(s.padStart(6, "*"));
console.log(s.padStart(7, "12"));
console.log(s.padEnd(6, "*-"));
console.log(s.padStart(2, "*")); // target < len → passthrough
console.log(s.padStart(6, "")); // empty pad → passthrough
console.log(w.padStart(8, "π"));
console.log(sub.padEnd(8, "!"));
console.log(s.padStart(-1));

console.log(s.repeat(0));
console.log(s.repeat(3));
console.log(w.repeat(2));
console.log(sub.repeat(2));
try {
  s.repeat(-1);
} catch (e) {
  console.log("repeat RangeError:", (e as Error).message);
}

console.log(s.concat("X"));
console.log(s.concat("X", "Y", "Z"));
console.log(s.concat());
console.log(s.concat(1, true));
console.log(sub.concat(w));

console.log(s.codePointAt(0));
console.log(s.codePointAt(5));
console.log(w.codePointAt(1));
console.log(w.codePointAt(2)); // astral lead → full code point
console.log(w.codePointAt(3)); // trail surrogate → lone
console.log(w.codePointAt());
console.log(sub.codePointAt(1));
console.log(s.codePointAt(-1));

console.log(s.localeCompare("abd"));
console.log(s.localeCompare("abc"));
console.log(s.localeCompare("abb"));
console.log(s.localeCompare("ab"));
console.log(sub.localeCompare("hello"));

console.log(s.padStart.name, s.padStart.length);
console.log(s.repeat.name, s.repeat.length);
console.log(s.codePointAt.name, s.codePointAt.length);
console.log(s.localeCompare.name, s.localeCompare.length);
console.log(s.concat.name, s.concat.length);
