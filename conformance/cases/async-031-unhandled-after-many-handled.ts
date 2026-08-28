// The pending-unhandled list drops entries that have already been
// observed once it needs to grow, so a program that handles many
// rejections does not pin them all until exit. The one rejection
// that is genuinely unobserved still has to survive that compaction
// and reach the listener.
process.on("unhandledRejection", (r) => {
  console.log("listener", r.message);
});

async function boom(): Promise<number> {
  throw new Error("handled");
}

async function main() {
  let caught = 0;
  for (let i = 0; i < 5000; i++) {
    try {
      await boom();
    } catch (e) {
      caught++;
    }
  }
  console.log("caught", caught);
  Promise.reject(new Error("still reported"));
  console.log("end");
}

main();
