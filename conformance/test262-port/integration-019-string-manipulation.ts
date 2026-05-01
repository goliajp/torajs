// Integration: heavy string manipulation. CSV-like parse + reformat,
// case fold, trim, and join. Exercises split + per-element transform
// + join interleavings.
function check(): number {
  // CSV-like split + clean.
  let raw = "  Alice ,  bob  , Charlie ,  ";
  let parts = raw.split(",").map((s: string): string => s.trim());
  if (parts.length !== 4) { throw "#1: " + parts.length; }
  if (parts[0] !== "Alice") { throw "#2"; }
  if (parts[1] !== "bob") { throw "#3"; }
  if (parts[2] !== "Charlie") { throw "#4"; }
  if (parts[3] !== "") { throw "#5"; }

  // Filter empty + uppercase + join.
  let kept = parts.filter((s: string): boolean => s.length > 0);
  let cleaned = kept.map((s: string): string => s.toUpperCase()).join("|");
  if (cleaned !== "ALICE|BOB|CHARLIE") { throw "#6"; }

  // String build via repeat + concat.
  let dashes = "-".repeat(10);
  if (dashes !== "----------") { throw "#7"; }

  // Multi-line concatenation simulation.
  let lines: string[] = ["line1", "line2", "line3"];
  let doc = lines.join("\n");
  if (doc.length !== 17) { throw "#8: 5 + 1 + 5 + 1 + 5"; }

  // String.charCodeAt + String.fromCharCode round-trip.
  let s = "AbCdEf";
  let codes: number[] = [];
  for (let i: number = 0; i < s.length; i = i + 1) {
    codes.push(s.charCodeAt(i));
  }
  let rebuilt: string[] = [];
  for (let c of codes) {
    rebuilt.push(String.fromCharCode(c));
  }
  if (rebuilt.join("") !== s) { throw "#9: roundtrip"; }

  // Case-insensitive contains: needle in haystack regardless of case.
  let hay = "The Quick Brown Fox";
  let needle = "QUICK";
  if (hay.toLowerCase().includes(needle.toLowerCase()) !== true) { throw "#10"; }

  // Pad-format columns.
  let entries: string[] = [];
  for (let i: number = 1; i <= 3; i = i + 1) {
    entries.push("[" + String(i).padStart(3, "0") + "]");
  }
  if (entries.join(" ") !== "[001] [002] [003]") { throw "#11"; }
  return 0;
}
console.log(check());
