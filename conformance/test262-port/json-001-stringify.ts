// Adapted from test262: built-ins/JSON/stringify/* — type-aware
// recursive serializer. Each call site is monomorphized inline based
// on the static type of the arg: primitives → direct formatter,
// strings → quote helper, arrays/structs → loop / static unfold +
// str_concat chain.
//
// v0 supports: number, string, boolean, array<T>, struct (homogeneous
// or heterogeneous fields). Out-of-scope: undefined / null fields,
// nested mixed-type arrays beyond Array<Array<T>>, custom toJSON,
// indent / replacer args.
type Pt = { x: number, y: number };
type Bag = { name: string, count: number, on: boolean };

function check(): number {
  // Primitives.
  if (JSON.stringify(42) !== "42") { throw "#1"; }
  if (JSON.stringify(-7) !== "-7") { throw "#2"; }
  if (JSON.stringify(0) !== "0") { throw "#3"; }
  if (JSON.stringify(true) !== "true") { throw "#4"; }
  if (JSON.stringify(false) !== "false") { throw "#5"; }
  if (JSON.stringify("hi") !== "\"hi\"") { throw "#6"; }
  if (JSON.stringify("") !== "\"\"") { throw "#7"; }

  // String escape: quotes, backslash, newline.
  if (JSON.stringify("a\"b") !== "\"a\\\"b\"") { throw "#8"; }
  if (JSON.stringify("a\\b") !== "\"a\\\\b\"") { throw "#9"; }
  if (JSON.stringify("a\nb") !== "\"a\\nb\"") { throw "#10"; }

  // Arrays — number / string / nested.
  if (JSON.stringify([1, 2, 3]) !== "[1,2,3]") { throw "#11"; }
  if (JSON.stringify(["a", "b"]) !== "[\"a\",\"b\"]") { throw "#12"; }

  let empty: number[] = [];
  if (JSON.stringify(empty) !== "[]") { throw "#13"; }

  let one: number[] = [42];
  if (JSON.stringify(one) !== "[42]") { throw "#14"; }

  let bools: boolean[] = [true, false, true];
  if (JSON.stringify(bools) !== "[true,false,true]") { throw "#15"; }

  // Structs.
  let p: Pt = { x: 5, y: 7 };
  if (JSON.stringify(p) !== "{\"x\":5,\"y\":7}") { throw "#16"; }

  let b: Bag = { name: "alice", count: 3, on: true };
  if (JSON.stringify(b) !== "{\"name\":\"alice\",\"count\":3,\"on\":true}") { throw "#17"; }

  // Array of structs.
  let pts: Pt[] = [{ x: 1, y: 2 }, { x: 3, y: 4 }];
  let s = JSON.stringify(pts);
  if (s !== "[{\"x\":1,\"y\":2},{\"x\":3,\"y\":4}]") { throw "#18"; }

  // Nested array.
  let grid: number[][] = [[1, 2], [3]];
  if (JSON.stringify(grid) !== "[[1,2],[3]]") { throw "#19"; }
  return 0;
}
console.log(check());
