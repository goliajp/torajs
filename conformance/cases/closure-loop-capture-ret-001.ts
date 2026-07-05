// RFC 20260705 chunk 556 — return-ann sniff binds collection was a
// flat top-level walk: a for-init let (`for (let i = 0; ...)`),
// block/loop-scoped lets and fn-body lets never entered the map, so
// an arrow capturing a loop variable kept the Void return default —
// "return type mismatch: function expects Void, got Number" at every
// constrained use.
function keep(f: (n: number) => number): (n: number) => number {
  return f;
}
let last = 0;
for (let i = 0; i < 3; i++) {
  let held = keep((n: number) => n * 3 + i);
  last = held(7);
}
console.log(last);

let direct = 0;
for (let i = 0; i < 3; i++) {
  let held = (n: number) => n * 3 + i;
  direct = held(7);
}
console.log(direct);

function inFn(): number {
  let acc = 0;
  for (let k = 10; k < 13; k++) {
    let f = (n: number) => n + k;
    acc = acc + f(1);
  }
  return acc;
}
console.log(inFn());

let words: string[] = [];
for (let j = 0; j < 2; j++) {
  let tag = (s: string) => s + "-tagged";
  words.push(tag("w"));
}
console.log(words[0]);
console.log(words[1]);
