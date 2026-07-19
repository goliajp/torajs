// ToPrimitive hint order on string concat: valueOf first for default hint (ES §7.1.1)
const o: any = {
  valueOf() { console.log("valueOf"); return 42; },
  toString() { console.log("toString"); return "str"; },
};
console.log("" + o);
console.log(o + "");
console.log(o + 1);
console.log(`${o}`);
console.log(String(o));

const t = {
  valueOf() { console.log("t-valueOf"); return 7; },
  toString() { console.log("t-toString"); return "typed"; },
};
console.log("" + t);
console.log(`${t}`);
const p = { toString() { return "onlyTS"; } };
console.log("" + p);
const q = {};
console.log("" + q);

// valueOf returning non-primitive falls through to toString
const r: any = {
  valueOf() { console.log("vo"); return {}; },
  toString() { console.log("ts"); return "fb"; },
};
console.log("" + r);

// number hint uses valueOf
const s: any = {
  valueOf() { return 1; },
  toString() { return "x"; },
};
console.log(+s);
console.log(s * 2);

// Date treats default hint as string, number hint stays numeric
const d: any = new Date(86400000);
console.log(typeof ("" + d) === "string", ("" + d).length > 10);
console.log(+d);
