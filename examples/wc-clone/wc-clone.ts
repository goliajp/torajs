// wc-clone — count lines, words, and bytes in a hardcoded text
// sample. Mirrors POSIX `wc` output format: "  L  W  B".
//
// Exercises: string iteration via charCodeAt, ASCII whitespace
// classification, multiple counters threaded through a single pass,
// integer-padded right-aligned output.

function isWhitespace(b: number): boolean {
  // ASCII space, tab, line-feed, carriage-return, vertical-tab,
  // form-feed. Anything else (incl. high-bit bytes) is a word char.
  return b === 0x20 || b === 0x09 || b === 0x0a || b === 0x0d || b === 0x0b || b === 0x0c;
}

function countWcc(text: string): number[] {
  let lines = 0;
  let words = 0;
  let bytes = 0;
  let inWord = false;
  for (let i = 0; i < text.length; i++) {
    const b = text.charCodeAt(i);
    bytes = bytes + 1;
    if (b === 0x0a) {
      lines = lines + 1;
    }
    if (isWhitespace(b)) {
      inWord = false;
    } else if (!inWord) {
      inWord = true;
      words = words + 1;
    }
  }
  return [lines, words, bytes];
}

function pad(s: string, width: number): string {
  let out = s;
  while (out.length < width) {
    out = " " + out;
  }
  return out;
}

function reportWcc(label: string, text: string): void {
  const counts = countWcc(text);
  console.log(
    pad(counts[0].toString(), 4) +
      pad(counts[1].toString(), 4) +
      pad(counts[2].toString(), 5) +
      " " +
      label
  );
}

const sample1 = "hello world\n";
const sample2 = "the quick brown fox\njumps over the lazy dog\n";
const sample3 =
  "one\ntwo  three\nfour five six\nseven eight nine ten\n";

reportWcc("sample1", sample1);
reportWcc("sample2", sample2);
reportWcc("sample3", sample3);
