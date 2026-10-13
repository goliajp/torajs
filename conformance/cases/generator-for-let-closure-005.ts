// a for-let closure made in a switch discriminant, with a suspend in a case body, keeps its iteration, in a generator and an async function
function* g() {
  const fs: Array<() => number> = [];
  for (let i = 0; i < 3; i++) {
    switch ((fs.push(() => i), i)) {
      case 1:
        yield i;
        break;
      default:
        break;
    }
  }
  yield fs.map((f) => f()).join(",");
}
console.log([...g()].join(" "));
async function a(): Promise<string> {
  const fs: Array<() => number> = [];
  for (let i = 0; i < 3; i++) {
    switch ((fs.push(() => i * 10), i)) {
      case 1:
        await null;
        break;
      default:
        break;
    }
  }
  return fs.map((f) => f()).join(",");
}
a().then(console.log);
