// Hex-valued escapes cook inside a template the same as in a string
// literal (§12.9.6 TV), and the raw strings keep the spelling (TRV).
console.log(`A\x42${1}\u{43}`, `A\x42${1}\u{43}`.length);
console.log(`\u{1F600}` === "😀", `😀`.length, `\uD800`.length, `\uD800`.charCodeAt(0));
console.log(`\u{000000041}` === "A", "\u{000000041}" === "A");
const raw = String.raw`aA\x42\u{43}b`;
console.log(raw, raw.length);
const tag = (s: any, ...v: any[]): string => s.raw.join("|") + "/" + s.join("|");
console.log(tag`xA${0}\x42y`);
