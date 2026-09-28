// an indexed read whose receiver is an owned temporary: the element
// must outlive the receiver, the receiver must be released, and a
// throw on the read must not leave the receiver behind
function nums(i: number): number[] { return [i, i * 2]; }
function strs(i: number): string[] { return ["a" + i, "b"]; }
function anys(i: number): any[] { return [i, "s" + i, { k: i }]; }
const xs = [1, 2, 3];
const ys = ["x", "y"];
let acc = 0;
let caught = 0;
for (let i = 0; i < 2000; i++) {
  const p = ("q" + i + "-r").split(",")[0];
  const q = anys(i)[1];
  const c: any = { a: "x" + i, b: 2 };
  const v = Object.values(c)[0];
  const k = Object.keys(c)[1];
  const named = xs[i % 3] + ys[i % 2].length;
  const opt = strs(i)?.[0];
  const view = ("a,b").split(",")[i % 2];
  acc += p.length + String(q).length + String(v).length + k.length + named + opt.length + view.length;
  acc += ("q" + i + "-r").split(",")[0].length + [i, i + 1][i & 1] + ["a" + i, "b"][i & 1].length;
  acc += nums(i)[0] + strs(i)[0].length + anys(i)[2].k;
  if (nums(i)[5] === undefined) caught++;
  try { acc += strs(i)[7].length; } catch (e) { caught++; }
}
console.log(acc, caught);
const m = ("a,b,c").split(",");
console.log(m[0], ("x,y").split(",")[1], [10, 20][1], ["p", "q"][0], String(Object.values({ k: "v" } as any)[0]));
console.log(strs(3)[0], nums(4)[1], anys(5)[1], anys(6)?.[0], ("u,v").split(",")?.[1]);
