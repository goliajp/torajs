// A write through an `any` receiver changes a struct's member only
// when the receiver can hold that struct's cell. A receiver that holds
// a fresh dynamic object takes the write as a property of its own; one
// that may alias a struct (an assignment, a parameter, a ternary, a
// reviver's answer) leaves the struct's member holding the value.

type P = { x: number; y: number };
const argc = process.argv.length;

// a fresh object: the struct members keep their numbers
const cfg: any = {};
cfg.x = argc > 99 ? "big" : "small";
cfg["y"] = "why";
Reflect.set(cfg, "x", "reflected");
const tmp: any = { z: 1 };
tmp.x = "z-side";
let later: any;
later = JSON.parse('{"w":1}');
later.x = "parsed";
const base: P = { x: 1.5, y: 2 };
let s = 0;
for (let i = 0; i < 4; i++) s += base.x + base.y + i;
console.log(s, cfg.x, cfg.y, tmp.x, later.x);

// a receiver that may alias the struct
const a: P = { x: 1, y: 2 };
let alias: any = {};
alias = a;
alias.x = "aliased";
console.log(typeof a.x, a.x, a.y);

const b: P = { x: 3, y: 4 };
const pick: any = argc > 99 ? {} : b;
pick.y = "picked";
console.log(typeof b.y, b.y, b.x);

const c: P = { x: 5, y: 6 };
function poke(o: any, v: any): void {
  o.x = v;
}
poke(c, "poked");
console.log(typeof c.x, c.x);

const d: P = { x: 7, y: 8 };
const revived: any = JSON.parse("{}", (_k, _v) => d);
revived.y = "revived";
console.log(typeof d.y, d.y);

const e: P = { x: 9, y: 10 };
const seq: any = (0, e);
Reflect.set(seq, "x", "reflected");
console.log(typeof e.x, e.x);

const lit: any = { x: 11, y: 12 };
lit.x = "own";
console.log(lit.x, lit.y);
