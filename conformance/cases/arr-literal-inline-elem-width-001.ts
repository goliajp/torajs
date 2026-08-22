// An array literal's element width is a fact about the literal, so
// it seeds whether or not the literal is bound to anything. The
// container walker only reaches literals standing in container
// positions, so one handed straight to a call took its element type
// from the first element — `[1, 2.5]` laid out I64 slots and read
// the fractional one back as its own bits.
console.log(JSON.stringify([1, 2.5]));
console.log(JSON.stringify([1, 2.5, 3]));
console.log(JSON.stringify([2.5, 1]));
console.log(JSON.stringify([[1, 2.5]]));
console.log(JSON.stringify({ x: [1, 2.5] }));
console.log(String([1, 2.5]));
console.log([1, 2.5].join(","));
console.log([1, 2.5][1]);

// Bound literals kept working through the container walker; they
// must still agree.
const bound = [1, 2.5, -3];
console.log(JSON.stringify(bound), String(bound), bound[1]);

// All-integral literals stay narrow.
console.log(JSON.stringify([1, 2, 3]), JSON.stringify([[4], [5]]));

// Round trip through the parser.
const back: number[] = JSON.parse(JSON.stringify([1, 2.5, -3]));
console.log(back[0], back[1], back[2]);

// Nested and spread.
const src = [0.5, 1];
console.log(JSON.stringify([...src, 2]));
console.log(JSON.stringify([1, ...src]));

// A computed fractional element, and a negative one.
console.log(JSON.stringify([1, 0.1 + 0.2]));
console.log(String([1, -0.5]));
