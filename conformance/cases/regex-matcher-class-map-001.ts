// a compiled matcher over latin-1 bytes tests a class of more than
// two ranges against one bit map. every byte must land on the side
// the class puts it: ascii word characters, the latin-1 letters a
// property names, and the bytes just outside each range.
const subjects = [
  "hello_World 42",
  "  x9_ab-cd",
  "çava, élan! ×÷ ªµº ÀÖØöøÿ",
  "@[`{/:_09azAZ",
  "",
  "wordy word sword word_ word",
];

function show(re: RegExp, s: string): string {
  const m = re.exec(s);
  return m === null ? "null" : m.index + ":" + m[0];
}

for (const s of subjects) {
  console.log(
    [
      show(/\w+/, s),
      show(/[0-9a-fA-F_]+/, s),
      show(/(\p{L}+)\P{L}/u, s),
      show(/\bword\b/, s),
      show(/[^\w\s]+/, s),
      s.replace(/[A-Za-z0-9]+/g, "#"),
      String(/^[\w@`]+$/.test(s)),
    ].join(" | "),
  );
}

let bytes = "";
for (let c = 0; c < 256; c++) bytes += String.fromCharCode(c);
let word = "";
for (const m of bytes.matchAll(/\w/g)) word += m[0];
console.log(word);
let letters = 0;
for (const m of bytes.matchAll(/\p{L}/gu)) letters += m[0].charCodeAt(0);
console.log(letters);
