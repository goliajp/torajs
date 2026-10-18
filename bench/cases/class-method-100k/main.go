// suppressed: both per-iteration allocations are stored to package-level sinks so they stay on the heap, as black_box keeps them in the rust row
package main

import (
	"fmt"
	"os"
	"strconv"
)

type Vec2 struct {
	x int64
	y int64
}

// heap-allocated like generic-pair-1m, since new Vec2 heap-allocates in main.ts
func newVec2(x, y int64) *Vec2 {
	return &Vec2{x: x, y: y}
}

func (v *Vec2) norm() int64 {
	return v.x*v.x + v.y*v.y
}

func (v *Vec2) scale(k int64) *Vec2 {
	return newVec2(v.x*k, v.y*k)
}

var sinkV, sinkS *Vec2

func main() {
	var total int64 = 0
	n, _ := strconv.ParseInt(os.Args[1], 10, 64)
	for i := int64(0); i < n; i++ {
		sinkV = newVec2(i, i+1)
		sinkS = sinkV.scale(2)
		total = total + sinkS.norm()
	}
	fmt.Println(total)
}
