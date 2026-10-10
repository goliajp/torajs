// `const v: T = JSON.parse(text)` with a user-written T: the text
// parses first (a SyntaxError when it is not JSON), and only then is
// the value checked against T, as a cast of the user's own type: a
// value without T's shape is a TypeError at the declaration. Each
// probe below reads the value the way that makes bun throw a
// TypeError too, so the two print the same line; tr throws earlier.
//
// A class slot: a parsed object with the class's fields becomes an
// instance of the class (any-into-class-slot-dynobj-001); one without
// them is the TypeError. bun does not throw at the declaration (the
// annotation is erased there); the probe calls a method, so bun throws
// the same TypeError one statement later.
class P {
  x: number;
  constructor(x: number) {
    this.x = x;
  }
  m(): number {
    return this.x + 1;
  }
}
const pText = process.argv.length > 100 ? "" : '{"y":5}';
try {
  const p: P = JSON.parse(pText);
  console.log(p.m());
} catch (e) {
  console.log("class slot", e instanceof TypeError);
}
// an array of a class: the empty array fits, an element without the
// class's fields does not
function classArr(t: string): void {
  try {
    const ps: P[] = JSON.parse(t);
    console.log("class array", ps.length, ps.length > 0 ? ps[0].m() : 0);
  } catch (e) {
    console.log("class array", e instanceof TypeError);
  }
}
classArr("[]");
classArr('[{"y":1}]');

// An interface slot: a structurally complete object is a legal value,
// whatever the order of its keys and whatever else it carries.
interface Q {
  x: number;
  y: string;
}
interface N {
  q: Q;
  tag: string;
}
function nested(t: string): void {
  try {
    const n: N = JSON.parse(t);
    console.log("nested", n.q.x.toFixed(1), n.q.y.length, n.tag.toUpperCase());
  } catch (e) {
    console.log("nested", e instanceof TypeError, e instanceof SyntaxError);
  }
}
nested('{"q":{"x":1,"y":"ab"},"tag":"t"}');
// keys in another order, and keys the type does not declare
nested('{"tag":"t","q":{"y":"ab","x":1.25}}');
nested('{"q":{"x":2,"y":"abc","z":true},"tag":"u","more":[1,{"k":null}]}');
nested(' { "tag" : "w" , "q" : { "y" : "" , "x" : -3e2 } } ');
// a missing field is a TypeError, caught where it is declared
nested('{"q":{"x":1},"tag":"t"}');
nested('{"q":{},"tag":"t"}');
nested('{"tag":"t"}');
// a field of another kind, at either depth
nested('{"q":{"x":"1","y":"ab"},"tag":"t"}');
nested('{"q":{"x":1,"y":"ab"},"tag":5}');
nested('{"q":3,"tag":"t"}');
nested('{"q":null,"tag":"t"}');
nested("[]");
// text that is not JSON is a SyntaxError, even where a field before
// the error already failed the type
nested('{"q":{"x":1,"y":"a"}');
nested('{"q":{"x":1,"y":"a"},"tag":');
nested('{"q":{"x":1},"tag":');
nested('{"tag":"t","q":{"x":"1"},}');
nested('{"q":{"y":"a","x":1},"tag":"t"} x');

// an array field
interface R {
  xs: number[];
  name: string;
}
function arrField(t: string): void {
  try {
    const r: R = JSON.parse(t);
    console.log("array field", r.xs.map((v) => v.toFixed(1)).join(","), r.name.length);
  } catch (e) {
    console.log("array field", e instanceof TypeError, e instanceof SyntaxError);
  }
}
arrField('{"xs":[1,2,3],"name":"a"}');
arrField('{"name":"ab","xs":[1.5,-2]}');
arrField('{"name":"ab","xs":[]}');
arrField('{"xs":[1,"2"],"name":"a"}');
arrField('{"xs":5,"name":"a"}');
arrField('{"xs":[1,2,"name":"a"}');

// an array of objects at the top
function objArr(t: string): void {
  try {
    const qs: Q[] = JSON.parse(t);
    console.log("object array", qs.map((q) => q.x.toFixed(0) + q.y.toUpperCase()).join(","));
  } catch (e) {
    console.log("object array", e instanceof TypeError, e instanceof SyntaxError);
  }
}
objArr('[{"x":1,"y":"a"},{"x":2,"y":"b"}]');
objArr('[{"y":"a","x":1},{"x":2,"y":"b","z":0}]');
objArr('[{"x":1}]');
objArr('[{"x":1,"y":"a"},');

// a scalar slot
function num(t: string): void {
  try {
    const n: number = JSON.parse(t);
    console.log("number", n.toFixed(2));
  } catch (e) {
    console.log("number", e instanceof TypeError, e instanceof SyntaxError);
  }
}
num("4.5");
num('"5"');
num("[4]");

// the error is a real TypeError, delivered before the next statement
function first(t: string): string {
  const q: Q = JSON.parse(t);
  return q.y.toUpperCase();
}
for (const t of ['{"x":1,"y":"ok"}', '{"x":2}', '{"y":"ab","x":3}']) {
  try {
    console.log(first(t));
  } catch (e) {
    console.log("first", e instanceof TypeError, typeof e);
  }
}

// a value built by the any-lane retry is an ordinary value of its type
const late: N = JSON.parse('{"tag":"z","q":{"y":"yy","x":7}}');
late.q.x += 1;
late.tag = late.tag + late.q.y;
console.log(late.q.x, late.tag, late.q.y.length);
console.log("end");
