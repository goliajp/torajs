// a merged declaration adds its members where it stands: reads of the
// object between the two declarations see only the first one's
// members; merging stays within one block, and a merged declaration
// may hold computed members referring to the earlier ones
enum Phase {
  Init = 1,
  Run,
}
console.log(Object.keys(Phase).join(","));

function seed(): number {
  return 20;
}

enum Phase {
  Stop = seed(),
  Done = Stop + Run,
}
console.log(Object.keys(Phase).join(","));
console.log(Phase.Init, Phase.Run, Phase.Stop, Phase.Done, Phase[20], Phase[22]);

function inner(): string {
  enum Local {
    X = 3,
    Y,
  }
  const before = Object.keys(Local).length;
  enum Local {
    Z = "zed",
  }
  return before + ":" + Object.keys(Local).join(",") + ":" + Local.Z + Local[4];
}
console.log(inner());

function siblings(flag: boolean): string {
  if (flag) {
    enum S {
      A = 1,
    }
    return Object.keys(S).join(",");
  } else {
    enum S {
      B = 2,
    }
    return Object.keys(S).join(",");
  }
}
console.log(siblings(true), siblings(false));

let p: Phase = Phase.Done;
p = Phase.Init;
console.log(p, Phase[p]);
