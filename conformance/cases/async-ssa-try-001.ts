// Awaits in every part of a `try`: the try block, the catch block and
// the finally block, a rejection caught after an await, a `return`
// and a `break` and a `continue` that run a finally body which awaits,
// a return or a throw a finally body overrides after awaiting, and
// nested statements. Strings and objects live across every await.

function later<T>(v: T): Promise<T> {
  return Promise.resolve(v);
}

function fail(msg: string): Promise<number> {
  return Promise.reject(new Error(msg));
}

class Box {
  constructor(public label: string) {}
}

async function everywhere(n: number): Promise<string> {
  const pre = "pre" + n;
  const box = new Box("box" + n);
  try {
    const a = await later(n * 2);
    if (n === 3) {
      await fail("t" + n);
    }
    return pre + ":" + a + ":" + box.label;
  } catch (e) {
    const m = await later((e as Error).message);
    return pre + ":caught:" + m + ":" + box.label;
  } finally {
    const f = await later("fin" + n);
    console.log("finally", f, pre, box.label);
  }
}

async function returnThroughFinally(): Promise<string> {
  const s = "body-" + "value";
  try {
    return s;
  } finally {
    await later(0);
    console.log("finally after the return awaited");
  }
}

async function finallyOverridesReturn(): Promise<string> {
  try {
    return "body";
  } finally {
    const v = await later("finally");
    return v;
  }
}

async function finallySwallowsThrow(): Promise<string> {
  try {
    throw new Error("lost");
  } finally {
    await later(1);
    return "swallowed";
  }
}

async function finallyKeepsThrow(): Promise<string> {
  try {
    await fail("kept");
    return "no";
  } finally {
    await later(1);
    console.log("finally before the rethrow");
  }
}

async function breakAndContinue(): Promise<number> {
  let i = 0;
  let sum = 0;
  for (; i < 6; i++) {
    try {
      if (i === 1) continue;
      if (i === 4) break;
      sum += await later(i);
    } finally {
      const label = "fin" + i;
      await later(label);
      console.log(label, sum);
    }
  }
  return i * 100 + sum;
}

async function nested(): Promise<string> {
  const out: string[] = [];
  try {
    try {
      out.push("inner " + (await later("a")));
      await fail("deep");
    } finally {
      out.push("inner finally " + (await later("b")));
    }
  } catch (e) {
    out.push("outer catch " + (e as Error).message + " " + (await later("c")));
  }
  try {
    out.push("again " + (await later("d")));
  } catch (e) {
    out.push("unreached");
  }
  return out.join(" | ");
}

async function catchParamAcrossAwait(): Promise<string> {
  try {
    await fail("param");
    return "no";
  } catch (e) {
    await later(0);
    await later(1);
    return (e as Error).message + "!";
  }
}

async function main(): Promise<void> {
  for (let n = 1; n <= 3; n++) {
    console.log(await everywhere(n));
  }
  console.log(await returnThroughFinally());
  console.log(await finallyOverridesReturn());
  console.log(await finallySwallowsThrow());
  try {
    await finallyKeepsThrow();
  } catch (e) {
    console.log("rejected", (e as Error).message);
  }
  console.log(await breakAndContinue());
  console.log(await nested());
  console.log(await catchParamAcrossAwait());
}

main();
