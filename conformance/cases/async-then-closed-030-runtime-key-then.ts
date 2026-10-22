// A computed key that holds "then" at run time, next to loop counters
// that do clear: the counters do not close the program, and the object
// written under the run-time key is awaited as a thenable.

type Plain = { a: number };

const parts = ["t", "h", "e", "n"];
let k = "";
for (let i = 0; i < parts.length; i++) {
  k = k + parts[i];
}
const box: Plain = { a: 5 };
const o: any = box;
o[k] = (res: (v: number) => void) => res(7);

async function main() {
  const v = await box;
  console.log(typeof v, String(v));
  const xs = [box, { a: 6 }];
  for (let i = 0; i < xs.length; i++) {
    const w = await xs[i];
    console.log(i, typeof w, String(w));
  }
}

main();
