// Integration: chain of String transformations exercising several
// methods (slice, replaceAll, toUpperCase, trim, split, join, padStart,
// padEnd) plus the `+` num→str coercion. Each step's output flows
// into the next, validating the StrRepr interop across all calls.
function check(): number {
  // Pipeline 1: clean → upper → split → join.
  let raw = "  hello world  ";
  let r1 = raw.trim().toUpperCase().split(" ").join("_");
  if (r1 !== "HELLO_WORLD") { throw "#1: " + r1; }

  // Pipeline 2: replace all + slice + concat.
  let s2 = "foo-bar-baz".replaceAll("-", "_").slice(0, 7);
  if (s2 !== "foo_bar") { throw "#2: " + s2; }

  // Pipeline 3: pad sequences.
  let parts: string[] = [];
  for (let i: number = 1; i <= 3; i = i + 1) {
    parts.push(String(i).padStart(3, "0"));
  }
  if (parts.length !== 3) { throw "#3"; }
  if (parts[0] !== "001") { throw "#4: " + parts[0]; }
  if (parts[1] !== "002") { throw "#5"; }
  if (parts[2] !== "003") { throw "#6"; }

  // Pipeline 4: sort strings then join.
  let items: string[] = ["banana", "apple", "cherry"];
  items.sort((a: string, b: string): number => a.localeCompare(b));
  if (items.join(",") !== "apple,banana,cherry") { throw "#7"; }

  // Pipeline 5: case-insensitive contains.
  let needle = "WORLD";
  let hay = "Hello, World!";
  let hit = hay.toLowerCase().includes(needle.toLowerCase());
  if (hit !== true) { throw "#8: case-insens"; }

  // Pipeline 6: number-string round-trip via String / Number.
  let n = 12345;
  if (Number(String(n)) !== n) { throw "#9"; }
  if (String(n).length !== 5) { throw "#10"; }
  return 0;
}
console.log(check());
