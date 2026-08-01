// break / continue escaping a try/finally runs F on the way out
// (§14.13.3 abrupt completion routes through finally)
let log1: string[] = [];
function* g(): Generator<any> {
  while (true) {
    try {
      yield 1;
      break;
    } finally {
      log1.push("fb");
    }
  }
  yield 2;
}
const it = g();
console.log(it.next().value);
console.log(it.next().value);
console.log(log1.join(","));

// continue escaping the try re-enters the loop after F
let log2: string[] = [];
function* h(): Generator<any> {
  for (let i = 0; i < 3; i = i + 1) {
    try {
      if (i == 1) {
        continue;
      }
      yield i;
    } finally {
      log2.push("fc" + i);
    }
  }
}
const it2 = h();
console.log(it2.next().value);
console.log(it2.next().value);
console.log(it2.next().done);
console.log(log2.join(","));

// nested finally chain: break crosses both, inside-out
let log3: string[] = [];
function* n(): Generator<any> {
  while (true) {
    try {
      try {
        yield "x";
        break;
      } finally {
        log3.push("inner");
      }
    } finally {
      log3.push("outer");
    }
  }
}
const it3 = n();
it3.next();
console.log(it3.next().done);
console.log(log3.join(","));

// a loop fully inside the try owns its own break — F runs only on
// natural completion, not per inner-break
let log4: string[] = [];
function* m(): Generator<any> {
  try {
    while (true) {
      yield 1;
      break;
    }
    yield 2;
  } finally {
    log4.push("fm");
  }
}
const it4 = m();
console.log(it4.next().value);
console.log(it4.next().value);
console.log(it4.next().done);
console.log(log4.join(","));
