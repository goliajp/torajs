// A `string[]` element slot can hold a live string, the undefined
// sentinel (an `any` undefined stored into the slot), or the
// Substr-shaped sentinel an out-of-range string index read answers.
// The guard in front of a member read on such an element used to be
// a runtime call on every read, once per character inside a loop
// whose receiver never changes; it is now the three inline compares,
// settled after the first read on the same path. Each nullish
// spelling still raises the TypeError at the first use.
function lens(xs: string[]): number {
  let total = 0;
  for (let i = 0; i < xs.length; i = i + 1) {
    const s = xs[i];
    for (let j = 0; j < s.length; j = j + 1) {
      total = total + s.charCodeAt(j);
    }
  }
  return total;
}
function firstCode(xs: string[]): number {
  return xs[1].charCodeAt(0);
}

console.log(lens(["ab", "cde", ""]));

const text = "xyz";
const oob: string[] = ["ab", text[7], "c"];
console.log(oob[1] === undefined, typeof oob[1]);
try {
  console.log(lens(oob));
} catch (e) {
  console.log((e as Error).name, e instanceof TypeError);
}
try {
  console.log(firstCode(oob));
} catch (e) {
  console.log((e as Error).name);
}

const u: string | undefined = undefined as any;
const undef: string[] = ["ab", u!, "c"];
console.log(undef[1] === undefined, typeof undef[1]);
try {
  console.log(lens(undef));
} catch (e) {
  console.log((e as Error).name, e instanceof TypeError);
}
try {
  console.log(undef[1].length);
} catch (e) {
  console.log((e as Error).name);
}
console.log(lens(["q"]), firstCode(["a", "b"]));
