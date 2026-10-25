// The receiver of an in-place sort, of Map / Set forEach and of
// Map getOrInsertComputed, read out of a field, lives through the
// callbacks even when a callback overwrites the field. `churn`
// reallocates cells of the same sizes, so a freed receiver would
// answer with another cell's contents.

class Box {
  xs: number[] = [];
  m: Map<number, number> = new Map();
  st: Set<number> = new Set();
}
const box = new Box();
const keepA: number[][] = [];
const keepM: Map<number, number>[] = [];
const keepT: Set<number>[] = [];
function churn(): void {
  for (let i = 0; i < 4; i++) {
    keepA.push([900 + i, 901, 902, 903]);
    const m = new Map<number, number>();
    m.set(900 + i, 900);
    m.set(901, 901);
    m.set(902, 902);
    keepM.push(m);
    const t = new Set<number>();
    t.add(900 + i);
    t.add(901);
    t.add(902);
    keepT.push(t);
  }
}

function main(): void {
  box.xs = [4, 3, 2, 1];
  const sorted = box.xs.sort((a: number, b: number): number => {
    box.xs = [];
    churn();
    return a - b;
  });
  console.log("sort", sorted.join(","), box.xs.length);
  box.m = new Map<number, number>();
  box.m.set(1, 10);
  box.m.set(2, 20);
  box.m.set(3, 30);
  let ms = 0;
  box.m.forEach((v: number, k: number): void => {
    box.m = new Map<number, number>();
    churn();
    ms += v * k;
  });
  console.log("map forEach", ms);
  box.st = new Set<number>();
  box.st.add(1);
  box.st.add(2);
  box.st.add(3);
  let ss = 0;
  box.st.forEach((v: number): void => {
    box.st = new Set<number>();
    churn();
    ss += v;
  });
  console.log("set forEach", ss);
  box.m = new Map<number, number>();
  box.m.set(1, 10);
  const got = box.m.getOrInsertComputed(3, (k: number): number => {
    box.m = new Map<number, number>();
    churn();
    return k * 100;
  });
  console.log("getOrInsertComputed", got, box.m.size);
}
main();
