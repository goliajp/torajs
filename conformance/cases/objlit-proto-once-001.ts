// one `__proto__: v` sets [[Prototype]]; shorthand, method and computed
// `__proto__` keys are ordinary own properties and may sit beside it,
// and an assignment pattern may name `__proto__` twice (bun 1.4.2
// refuses the shorthand beside `__proto__: v`, which §B.3.1 allows)
const p = { tag: "p" };
const __proto__ = 5;
const o: any = { __proto__: p, ["__proto__"]: 1 };
console.log(o.tag, Object.getPrototypeOf(o) === p, Object.keys(o).length);
const s: any = { __proto__: p, __proto__ };
console.log(s.tag, Object.keys(s).join(","));
const m: any = { __proto__: p, __proto__() { return 2; } };
console.log(m.tag, Object.keys(m).join(","));
let x: any;
let y: any;
({ __proto__: x, __proto__: y } = { tag: 1 });
console.log(x === Object.prototype, y === Object.prototype);
