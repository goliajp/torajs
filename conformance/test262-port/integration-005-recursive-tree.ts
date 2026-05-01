// Integration: recursive helper functions over a flat tree (parent
// indices) — exercises tail-position calls + mutual recursion. Pure
// computation, no allocation in the hot path.
function depth_of(parents: number[], i: number): number {
  if (parents[i] === -1) { return 0; }
  return 1 + depth_of(parents, parents[i]);
}

function is_descendant(parents: number[], child: number, ancestor: number): boolean {
  if (child === ancestor) { return true; }
  if (parents[child] === -1) { return false; }
  return is_descendant(parents, parents[child], ancestor);
}

function leaf_count(parents: number[]): number {
  let count = 0;
  let n = parents.length;
  for (let i: number = 0; i < n; i = i + 1) {
    let has_child: boolean = false;
    for (let j: number = 0; j < n; j = j + 1) {
      if (parents[j] === i) { has_child = true; break; }
    }
    if (!has_child) { count = count + 1; }
  }
  return count;
}

function check(): number {
  // Tree:    0
  //         / \
  //        1   2
  //       /|   |
  //      3 4   5
  //              \
  //               6
  let parents: number[] = [-1, 0, 0, 1, 1, 2, 5];

  if (depth_of(parents, 0) !== 0) { throw "#1: root"; }
  if (depth_of(parents, 3) !== 2) { throw "#2"; }
  if (depth_of(parents, 6) !== 3) { throw "#3"; }

  if (is_descendant(parents, 3, 0) !== true) { throw "#4: 3 desc 0"; }
  if (is_descendant(parents, 6, 2) !== true) { throw "#5: 6 desc 2"; }
  if (is_descendant(parents, 5, 1) !== false) { throw "#6: 5 not desc 1"; }
  if (is_descendant(parents, 0, 1) !== false) { throw "#7: root not desc"; }
  if (is_descendant(parents, 4, 4) !== true) { throw "#8: self"; }

  // Leaves are 3, 4, 6 → 3 of them.
  if (leaf_count(parents) !== 3) { throw "#9"; }
  return 0;
}
console.log(check());
