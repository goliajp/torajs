// Integration: recursive string algorithms — palindrome detection,
// anagram check, character histogram. Exercises String.charCodeAt,
// Array.fill (via init), and i/j double-loop patterns.
function is_palindrome(s: string): boolean {
  let lo = 0;
  let hi = s.length - 1;
  while (lo < hi) {
    if (s.charCodeAt(lo) !== s.charCodeAt(hi)) { return false; }
    lo = lo + 1;
    hi = hi - 1;
  }
  return true;
}

function is_anagram(a: string, b: string): boolean {
  if (a.length !== b.length) { return false; }
  // ASCII-only counts table.
  let ca: number[] = [];
  let cb: number[] = [];
  for (let i: number = 0; i < 128; i = i + 1) { ca.push(0); cb.push(0); }
  for (let i: number = 0; i < a.length; i = i + 1) {
    let pa = a.charCodeAt(i);
    let pb = b.charCodeAt(i);
    ca[pa] = ca[pa] + 1;
    cb[pb] = cb[pb] + 1;
  }
  for (let i: number = 0; i < 128; i = i + 1) {
    if (ca[i] !== cb[i]) { return false; }
  }
  return true;
}

function check(): number {
  if (is_palindrome("racecar") !== true) { throw "#1"; }
  if (is_palindrome("hello") !== false) { throw "#2"; }
  if (is_palindrome("a") !== true) { throw "#3: single char"; }
  if (is_palindrome("") !== true) { throw "#4: empty"; }
  if (is_palindrome("aa") !== true) { throw "#5"; }
  if (is_palindrome("ab") !== false) { throw "#6"; }

  if (is_anagram("listen", "silent") !== true) { throw "#7"; }
  if (is_anagram("hello", "world") !== false) { throw "#8"; }
  if (is_anagram("abc", "cab") !== true) { throw "#9"; }
  if (is_anagram("abc", "ab") !== false) { throw "#10: len mismatch"; }
  return 0;
}
console.log(check());
