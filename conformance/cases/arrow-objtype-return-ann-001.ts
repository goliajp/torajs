// An arrow whose return type is written as an object type literal
// (`(): { x: number } => …`) parses as an arrow, with an expression
// body or a block body; a ternary whose else-arm is an object literal
// still parses as a ternary.
const make = (x: number): { x: number; y: string } => ({ x, y: "y" + x });
const nested = (): { p: { q: number[] } } => {
  return { p: { q: [1, 2] } };
};
const none = (): {} => ({});
const c = true;
const t = c ? ({ z: 1 }) : { z: 2 };
console.log(make(3).x, make(4).y, nested().p.q.length, JSON.stringify(none()), t.z);
