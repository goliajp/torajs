// Phase M6.3 — JSON.parse, end-to-end. Caller-driven typing:
// `let v: T = JSON.parse(text)` reads the slot's annotation `T` at
// lower time and emits per-shape recursive parser calls into the
// runtime helpers (`__torajs_json_parse_int` / `_float` / `_bool` /
// `_string` plus `__torajs_json_eat_char` / `_arr_step` /
// `_arr_first` / `_str_eq_cstr`). Cursor is a single alloca'd i64
// threaded through all recursion. Syntactic mismatch triggers
// `__torajs_throw_set` and propagates via the existing throw
// infrastructure.
//
// Coverage exercised here:
//   - primitives:  number / boolean / string (with escape decode)
//   - homogeneous arrays of every primitive
//   - empty array / single-element / multi-element
//   - struct with mixed-type fields
//   - struct with nested struct field
//   - struct with array field, including string-element array
//
// Three-way agreement (bun + tr-jit + tr-aot) verified via the
// conformance runner.

type Pt = { x: number, y: number };
type User = { name: string, age: number, active: boolean };
type Bag = { id: number, tags: string[] };
type Nest = { p: Pt, label: string };

function check(): number {
  let n: number = JSON.parse("42");
  if (n !== 42) { throw "#1: int"; }

  let neg: number = JSON.parse("-7");
  if (neg !== -7) { throw "#2: negative int"; }

  let b1: boolean = JSON.parse("true");
  if (!b1) { throw "#3: true"; }
  let b2: boolean = JSON.parse("false");
  if (b2) { throw "#4: false"; }

  let s: string = JSON.parse("\"hi\"");
  if (s !== "hi") { throw "#5: ascii string"; }

  let esc: string = JSON.parse("\"a\\\"b\\nc\"");
  if (esc !== "a\"b\nc") { throw "#6: escape decode"; }

  let xs: number[] = JSON.parse("[1, 2, 3]");
  if (xs.length !== 3) { throw "#7: arr length"; }
  if (xs[0] !== 1 || xs[1] !== 2 || xs[2] !== 3) { throw "#8: arr elems"; }

  let empty: number[] = JSON.parse("[]");
  if (empty.length !== 0) { throw "#9: empty arr"; }

  let one: string[] = JSON.parse("[\"only\"]");
  if (one.length !== 1 || one[0] !== "only") { throw "#10: single-elem arr"; }

  let pt: Pt = JSON.parse("{\"x\": 7, \"y\": 11}");
  if (pt.x !== 7 || pt.y !== 11) { throw "#11: simple struct"; }

  let u: User = JSON.parse("{\"name\":\"alice\",\"age\":30,\"active\":true}");
  if (u.name !== "alice") { throw "#12: struct.name"; }
  if (u.age !== 30) { throw "#13: struct.age"; }
  if (!u.active) { throw "#14: struct.active"; }

  let bag: Bag = JSON.parse("{\"id\":42,\"tags\":[\"x\",\"y\",\"z\"]}");
  if (bag.id !== 42) { throw "#15: bag.id"; }
  if (bag.tags.length !== 3) { throw "#16: bag.tags.length"; }
  if (bag.tags[0] !== "x" || bag.tags[2] !== "z") { throw "#17: bag.tags"; }

  let nest: Nest = JSON.parse("{\"p\":{\"x\":3,\"y\":4},\"label\":\"go\"}");
  if (nest.p.x !== 3 || nest.p.y !== 4) { throw "#18: nest.p"; }
  if (nest.label !== "go") { throw "#19: nest.label"; }

  return 0;
}
console.log(check());
