// An arrow stored into an `Array<F>` takes its parameter types from F,
// exactly as it does from the `F[]` spelling of the same type. Every
// call through the array passes the declared types, so a parameter
// left untyped read the number it was handed as something else.
const k: number[] = [0];
const ws: Array<(v: number) => void> = [];
ws.push((v) => { k[0] = v; });
ws[0](42);
console.log(k[0]);

ws.unshift((v) => { k[0] = v * 2; });
ws[0](21);
console.log(k[0], ws.length);

ws[2] = (v) => { k[0] = v + 0.5; };
ws[2](1);
console.log(k[0], ws.length);

const lit: Array<(a: number, b: string) => string> = [(a, b) => b + (a + 1)];
console.log(lit[0](1, "n="));

const fs: Array<(x: number) => number> = [];
fs.push((x) => x * x, (x) => x / 4);
console.log(fs.map((f) => f(3)).join(","));

const strs: Array<(s: string) => number> = [];
strs.push((s) => s.length);
console.log(strs[0]("hello"));

type Box = { hs: Array<(v: number) => number> };
const box: Box = { hs: [] };
box.hs.push((v) => v - 1);
box.hs.push((v) => v * 10);
console.log(box.hs[0](5), box.hs[1](5));
