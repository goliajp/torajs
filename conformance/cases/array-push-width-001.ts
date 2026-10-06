// A RegExp's lastIndex (§22.2.6.13) pushed into, stored into and
// summed through a `number[]`: the element slot has to hold the
// double the read produces.

const idx: number[] = [];
const all = /b/g;
while (all.test("abab")) idx.push(all.lastIndex);
console.log(idx.join(","), idx.length);

let sum = 0;
for (const v of idx) sum += v;
console.log(sum, idx.reduce((a, b) => a + b, 0));

const ends: number[] = [];
const word = /\w+/g;
while (word.exec("ab cde f") !== null) {
  ends.push(word.lastIndex);
  ends.push(word.lastIndex / 4);
}
console.log(ends.join(" "), ends[1] + ends[3]);

const slots: number[] = [0, 0];
const y = /x/y;
y.lastIndex = 2;
y.test("abxx");
slots[0] = y.lastIndex;
slots[1] = Math.PI;
console.log(slots.join(","), slots[0] * 2);
