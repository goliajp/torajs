// Every string builder stops at the same longest-string bound and
// raises a RangeError instead of building past it. Each case is caught
// before anything near that size is allocated.

function tooLong(label: string, build: () => string): void {
  try {
    const s = build();
    console.log(label, "built", s.length);
  } catch (e) {
    console.log(label, e instanceof RangeError);
  }
}

const k64 = "x".repeat(1 << 16);
const y64 = "y".repeat(1 << 16);
const mb = "z".repeat(1 << 20);
const parts: string[] = [];
for (let i = 0; i < 2048; i++) parts.push(mb);

tooLong("repeat", () => "ab".repeat(2 ** 30));
tooLong("repeat-wide", () => "中".repeat(2 ** 31));
tooLong("repeat-wrap", () => k64.repeat(1 << 16));
tooLong("replaceAll-fn", () => k64.replaceAll("x", () => y64));
tooLong("padStart", () => "a".padStart(2 ** 31));
tooLong("padEnd", () => "a".padEnd(2 ** 31, "xy"));
tooLong("join", () => parts.join(""));
tooLong("join-sep", () => parts.slice(0, 1025).join(mb));

// at or under the bound, or nothing to build: no error
tooLong("repeat-empty", () => "".repeat(2 ** 40));
tooLong("pad-empty-filler", () => "a".padStart(2 ** 31, ""));
tooLong("join-small", () => parts.slice(0, 4).join("-"));
tooLong("replaceAll-small", () => "abc".replaceAll("b", y64));
