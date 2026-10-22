// A loop counter written by a destructuring and by a closure: either
// can put "then" into it, so it is no counter, and the key it names
// makes the object a thenable.

type Plain = { a: number };

const word = ["th", "en"].join("");
const a: Plain = { a: 1 };
const b: Plain = { a: 2 };
for (let i: any = 0; i < 1; i++) {
  [i] = [word];
  (a as any)[i] = (res: (v: string) => void) => res("from a");
}
for (let i: any = 0; i < 1; i++) {
  const put = () => {
    i = word;
  };
  put();
  (b as any)[i] = (res: (v: string) => void) => res("from b");
}

async function main() {
  console.log(String(await a), String(await b));
}

main();
