// Integration: JSON.stringify across nested structures. Validates
// that array-of-struct, struct-of-array, and deeply-nested combos
// produce spec-compatible JSON output.
type Item = { name: string, count: number, on: boolean };
type Box = { items: Item[], total: number };

function check(): number {
  // Primitives.
  if (JSON.stringify(0) !== "0") { throw "#1"; }
  if (JSON.stringify(-1) !== "-1") { throw "#2"; }
  if (JSON.stringify(true) !== "true") { throw "#3"; }
  if (JSON.stringify("") !== "\"\"") { throw "#4"; }

  // Arrays.
  let xs: number[] = [10, 20, 30];
  if (JSON.stringify(xs) !== "[10,20,30]") { throw "#5"; }
  let ys: string[] = ["a", "b"];
  if (JSON.stringify(ys) !== "[\"a\",\"b\"]") { throw "#6"; }

  // Nested.
  let nested: number[][] = [[1, 2], [3, 4]];
  if (JSON.stringify(nested) !== "[[1,2],[3,4]]") { throw "#7"; }

  // Struct.
  let it: Item = { name: "apple", count: 5, on: true };
  let s = JSON.stringify(it);
  if (s !== "{\"name\":\"apple\",\"count\":5,\"on\":true}") { throw "#8"; }

  // Array of structs.
  let items: Item[] = [
    { name: "apple", count: 5, on: true },
    { name: "banana", count: 3, on: false }
  ];
  let arr_s = JSON.stringify(items);
  let want = "[{\"name\":\"apple\",\"count\":5,\"on\":true},{\"name\":\"banana\",\"count\":3,\"on\":false}]";
  if (arr_s !== want) { throw "#9: " + arr_s; }

  // Struct with array field.
  let bx: Box = { items: items, total: 8 };
  let bx_s = JSON.stringify(bx);
  let bx_want = "{\"items\":" + want + ",\"total\":8}";
  if (bx_s !== bx_want) { throw "#10"; }

  // Escape sequences.
  let esc = "a\"b\\c\nd";
  if (JSON.stringify(esc) !== "\"a\\\"b\\\\c\\nd\"") { throw "#11: escape"; }
  return 0;
}
console.log(check());
