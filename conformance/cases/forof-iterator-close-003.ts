// §7.4.9 on a `return` out of a for-of body. `break` already closed
// (it branches to the loop's exit block, which is where the close
// lives); `return` leaves for the function epilogue and used to skip
// both the close AND the iterator slot's release.

let anyClosed = 0;

const anySrc: any = {
  [Symbol.iterator]() {
    let n = 0;
    return {
      next() {
        n = n + 1;
        return { value: n, done: n > 5 };
      },
      return() {
        anyClosed = anyClosed + 1;
        return { value: 0, done: true };
      },
    };
  },
};

function anyViaReturn(src: any): number {
  for (const v of src) {
    if (v === 2) {
      return v;
    }
  }
  return -1;
}

function anyRunsOut(src: any): number {
  let last = 0;
  for (const v of src) {
    last = v;
  }
  return last;
}

console.log("any return  :", anyViaReturn(anySrc), "closed:", anyClosed);
anyClosed = 0;
// running to completion is NOT an abrupt completion — no close
console.log("any full    :", anyRunsOut(anySrc), "closed:", anyClosed);

// the statically-typed lane reaches the same teardown by its own route
let typedClosed = 0;

class CountIter {
  n: number = 0;
  next(): { value: number; done: boolean } {
    this.n = this.n + 1;
    return { value: this.n, done: this.n > 5 };
  }
  return(): { value: number; done: boolean } {
    typedClosed = typedClosed + 1;
    return { value: 0, done: true };
  }
}

class Counted {
  [Symbol.iterator](): CountIter {
    return new CountIter();
  }
}

function typedViaReturn(s: Counted): number {
  for (const v of s) {
    if (v === 3) {
      return v;
    }
  }
  return -1;
}

console.log("typed return:", typedViaReturn(new Counted()), "closed:", typedClosed);

// a return out of nested loops unwinds both, innermost first
let nestedClosed = 0;

const outerSrc: any = {
  [Symbol.iterator]() {
    let n = 0;
    return {
      next() {
        n = n + 1;
        return { value: n, done: n > 3 };
      },
      return() {
        nestedClosed = nestedClosed + 1;
        return { value: 0, done: true };
      },
    };
  },
};

const innerSrc: any = {
  [Symbol.iterator]() {
    let n = 0;
    return {
      next() {
        n = n + 1;
        return { value: n, done: n > 3 };
      },
      return() {
        nestedClosed = nestedClosed + 1;
        return { value: 0, done: true };
      },
    };
  },
};

function nested(): string {
  for (const a of outerSrc) {
    for (const b of innerSrc) {
      if (a === 2 && b === 2) {
        return a + "," + b;
      }
    }
  }
  return "none";
}

console.log("nested      :", nested(), "closed:", nestedClosed);
