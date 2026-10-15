// §7.4.9 IteratorClose reads `return` with a [[Get]] on every abrupt
// exit from a for-of: a method is called with the iterator as this, a
// getter runs, a non-callable `return` is a TypeError on break but
// gives way to the body's own throw, and null / undefined close quietly.

const log: string[] = [];

function source(tag: string, ret: any, kind: string): any {
  return {
    [Symbol.iterator]() {
      let n = 0;
      const it: any = {
        next() {
          n = n + 1;
          return { value: n, done: n > 5 };
        },
      };
      if (kind === "getter") {
        Object.defineProperty(it, "return", {
          get() {
            log.push(tag + ":get");
            return ret;
          },
        });
      } else if (kind !== "none") {
        it.return = ret;
      }
      return it;
    },
  };
}

function closer(tag: string): any {
  return function (this: any) {
    log.push(tag + ":return:" + typeof this.next);
    return {};
  };
}

function breakAt(src: any, at: number): number {
  let last = 0;
  for (const v of src) {
    last = v;
    if (v === at) {
      break;
    }
  }
  return last;
}

function throwAt(src: any, at: number): string {
  try {
    for (const v of src) {
      if (v === at) {
        throw new Error("body " + v);
      }
    }
  } catch (e: any) {
    return e.constructor.name + ": " + e.message;
  }
  return "no throw";
}

function tryBreak(src: any, at: number): string {
  try {
    return "ok " + breakAt(src, at);
  } catch (e: any) {
    return e.constructor.name;
  }
}

console.log("method break :", breakAt(source("m", closer("m"), "method"), 2));
console.log("method throw :", throwAt(source("t", closer("t"), "method"), 3));
console.log("getter break :", breakAt(source("g", closer("g"), "getter"), 1));
console.log("getter throw :", throwAt(source("h", closer("h"), "getter"), 4));
console.log("number break :", tryBreak(source("n", 1, "value"), 2));
console.log("string break :", tryBreak(source("s", "x", "value"), 2));
console.log("object break :", tryBreak(source("o", {}, "value"), 2));
console.log("number throw :", throwAt(source("p", 1, "value"), 2));
console.log("getter num   :", tryBreak(source("q", 7, "getter"), 3));
console.log("null break   :", tryBreak(source("z", null, "value"), 2));
console.log("undef break  :", tryBreak(source("u", undefined, "value"), 2));
console.log("absent break :", tryBreak(source("a", 0, "none"), 2));

const thrower: any = {
  [Symbol.iterator]() {
    return {
      next() {
        return { value: 1, done: false };
      },
      get return(): any {
        throw new RangeError("from getter");
      },
    };
  },
};
console.log("getter throws:", tryBreak(thrower, 1));
console.log("getter vs body:", throwAt(thrower, 1));

let loops = 0;
for (let i = 0; i < 1000; i++) {
  loops = loops + breakAt(source("r", closer("r"), "method"), 3);
}
console.log("loops        :", loops, log.length);
console.log(log.slice(0, 12).join(" "));
