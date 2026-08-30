// A `for` / `while` loop whose body is nothing but `xs.push(_)` gets a
// reservation above the loop and appends by storing the slot directly.
// The running count lived only in a register until the loop's normal
// exit, which assumed nothing could read the array before then — but
// the shape constrains the push *statement*, not the push *argument*.
function selfRead(): string {
  const xs: number[] = [];
  for (let i = 0; i < 5; i = i + 1) {
    xs.push(xs.length);
  }
  return xs.join(",");
}
console.log(selfRead());

// The same read reached through a closure over the array.
function viaClosure(): string {
  const xs: number[] = [];
  const peek = (): number => xs.length;
  for (let i = 0; i < 5; i = i + 1) {
    xs.push(peek());
  }
  return xs.join(",");
}
console.log(viaClosure());

// Leaving by any edge other than the normal exit skipped the
// settlement entirely, so every append the loop had made went with it.
function thrownOut(): string {
  const xs: number[] = [];
  const stop = (i: number): number => {
    if (i === 3) {
      throw new Error("stop");
    }
    return i;
  };
  try {
    for (let i = 0; i < 9; i = i + 1) {
      xs.push(stop(i));
    }
  } catch (e) {
    /* the array outlives the loop that was filling it */
  }
  return xs.length + "|" + xs.join(",");
}
console.log(thrownOut());

// The `while` shape takes the same lowering, through its own lane.
function whileShape(): string {
  const xs: number[] = [];
  let i = 0;
  while (i < 5) {
    xs.push(xs.length);
    i = i + 1;
  }
  return xs.join(",");
}
console.log(whileShape());

// And the ordinary exits still answer what they always did.
function plain(): string {
  const xs: number[] = [];
  for (let i = 0; i < 4; i = i + 1) {
    xs.push(i * 2);
  }
  return xs.length + "|" + xs.join(",");
}
console.log(plain());

// The same shapes written `i++`. That spelling stopped matching the
// detector when postfix increment became its own AST node, so until
// now none of these took the fast lowering at all.
function selfReadPostfix(): string {
  const xs: number[] = [];
  for (let i = 0; i < 5; i++) {
    xs.push(xs.length);
  }
  return xs.join(",");
}
console.log(selfReadPostfix());

function thrownOutPostfix(): string {
  const xs: number[] = [];
  const stop = (i: number): number => {
    if (i === 3) {
      throw new Error("stop");
    }
    return i;
  };
  try {
    for (let i = 0; i < 9; i++) {
      xs.push(stop(i));
    }
  } catch (e) {
    /* swallow */
  }
  return xs.length + "|" + xs.join(",");
}
console.log(thrownOutPostfix());

function whilePostfix(): string {
  const xs: number[] = [];
  let i = 0;
  while (i < 5) {
    xs.push(xs.length);
    i++;
  }
  return xs.join(",");
}
console.log(whilePostfix());

function plainPostfix(): string {
  const xs: number[] = [];
  for (let i = 0; i < 4; i++) {
    xs.push(i * 2);
  }
  return xs.length + "|" + xs.join(",");
}
console.log(plainPostfix());
