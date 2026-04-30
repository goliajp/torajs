// Integration: string search wrapper around indexOf + includes.
function findOrThrow(s: string, sub: string): number {
  let idx: number = s.indexOf(sub);
  if (idx < 0) { throw "not found"; }
  return idx;
}

function check(): number {
  if (findOrThrow("hello world", "world") !== 6) { throw "#1"; }
  if (findOrThrow("abcabc", "b") !== 1) { throw "#2"; }
  let caught: string = "";
  try {
    findOrThrow("abc", "z");
  } catch (e: string) {
    caught = e;
  }
  if (caught !== "not found") { throw "#3"; }
  return 0;
}
console.log(check());
