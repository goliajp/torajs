// exit-code: 13
// `await` of a typed promise whose value is a Copy type (number,
// boolean) or a heap type (bigint) that is still pending once every
// queued job has run: nothing left can settle it. Inside an async
// function the function stays suspended there and its own result
// stays pending; the code after the await never runs on a value that
// does not exist, and the `catch` around it never runs either. At the
// module's top level the body never goes on (§16.2.1.5.3): the program
// ends with the job queue empty and exits 13 with a warning on stderr,
// as Node does for an unsettled top-level await. A settled promise
// still answers its value.
//
// bun spins on the last await, so the expected output is pinned beside
// this file.
function neverNum(): Promise<number> {
  return new Promise(() => {});
}
function neverBool(): Promise<boolean> {
  return new Promise(() => {});
}
function neverBig(): Promise<bigint> {
  return new Promise(() => {});
}

async function waitNum(): Promise<void> {
  try {
    const n: number = await neverNum();
    console.log("number after never", n);
  } catch (e) {
    console.log("number raised", e);
  }
}
async function waitBool(): Promise<void> {
  try {
    const b: boolean = await neverBool();
    console.log("boolean after never", b);
  } catch (e) {
    console.log("boolean raised", e);
  }
}
async function waitBig(): Promise<void> {
  const g: bigint = await neverBig();
  console.log("bigint after never", g);
}

const r: number = await Promise.resolve(3);
console.log("resolved", r);
let res: any;
const later: Promise<boolean> = new Promise((r) => {
  res = r;
});
queueMicrotask(() => res(true));
const t: boolean = await later;
console.log("settled by a job", t);

waitNum().then(() => console.log("waitNum settled"));
waitBool().then(() => console.log("waitBool settled"));
waitBig().then(() => console.log("waitBig settled"));
const again: number = await Promise.resolve(4);
console.log("resolved again", again);
try {
  const n: number = await neverNum();
  console.log("number after never", n);
} catch (e) {
  console.log("number raised", e);
}
console.log("unreachable");
