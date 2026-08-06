// `.trim()` on a Substr receiver takes the stack-write fast path,
// whose value is the buffer itself. That buffer IS the trimmed view,
// so it has to be typed as one: `.length` dispatches on the type, and
// so does the drop that releases the parent reference the trim took.
function rowLen(line: string): number {
  const parts = line.split(",");
  let total = 0;
  for (let i = 0; i < parts.length; i = i + 1) {
    const t = parts[i].trim();
    total = total + t.length;
  }
  return total;
}
console.log(rowLen("  alpha , beta , gamma  "));

// same shape without the loop, and reading the length inline
function firstLen(line: string): number {
  const parts = line.split(",");
  return parts[0].trim().length;
}
console.log(firstLen("  ab  , c"));

// the trimmed view must still read correctly, not just measure
function firstWord(line: string): string {
  const parts = line.split(",");
  return parts[0].trim();
}
console.log(firstWord("  hello  , x"), firstWord("  hello  , x").length);

// trimStart / trimEnd keep their own paths — pin them beside trim
function edges(line: string): string {
  const parts = line.split(",");
  return "[" + parts[0].trimStart() + "|" + parts[0].trimEnd() + "]";
}
console.log(edges("  pad  , y"));
