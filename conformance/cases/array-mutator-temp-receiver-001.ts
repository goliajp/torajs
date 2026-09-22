// An owned temporary array as a mutator receiver — an array literal or a
// call result: `[1, 2].push(3)`, `mk().pop()`, `[7].unshift(1)`. B1 keeps
// the cell across grow, so the temp needs no write-back; it is lowered
// once and released after the call (the loop would leak otherwise).
function mk(): number[] { return [1, 2]; }
function ms(): string[] { return ["a", "b"]; }
console.log([1, 2].push(3), mk().push(9), ms().push("c"));
console.log([1, 2, 3].pop(), mk().pop(), ms().pop(), [].pop());
console.log(mk().shift(), ms().shift(), [7].unshift(1), ms().unshift("z"));
const xs: any[] = [1, "x"];
console.log([...xs].pop(), [{ a: 1 }].pop()!.a);
let c = 0;
for (let i = 0; i < 3000; i++) { c += [i, i + 1].push(i) + mk().pop()! + ms().shift()!.length; }
console.log(c);
try { Object.freeze([1]).push(2); } catch (e) { console.log((e as any).constructor.name); }
