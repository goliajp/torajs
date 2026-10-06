// null / undefined written into a typed string field through any are
// seen by typeof, equality and nullish checks on the typed read
type C = { label: string; n: number };
const c: C = { label: "a", n: 1 };
const ac: any = c;
ac.label = null;
console.log(typeof c.label, c.label, c.label === null, c.label ?? "dflt");
ac.label = undefined;
console.log(typeof c.label, c.label === undefined, c.label ?? "dflt");
ac.label = "back";
console.log(typeof c.label, c.label, c.label.length);
const d: C = { label: "keep", n: 2 };
console.log(typeof d.label, d.label.length);
