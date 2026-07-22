// s?.[i] boxes a fresh Substr view; its only stake transfers into
// the box (rotation 184 — the old drop-then-box double release
// freed the view under the live box).
const s: string = "hello";
const c = s?.[1];
console.log(c, typeof c);
const d = s?.[99];
console.log(d);
let acc = "";
for (let i = 0; i < 5; i++) { const ch = s?.[i]; acc += ch; }
console.log(acc);
