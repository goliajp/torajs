// labeled block break (non-loop label)
let log: string[] = [];
blk: {
  log.push("a");
  if (log.length > 0) break blk;
  log.push("b");
}
log.push("c");
console.log(log.join(","));
