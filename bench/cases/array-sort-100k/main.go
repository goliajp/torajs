package main

import (
	"fmt"
	"os"
	"slices"
	"strconv"
)

func build(n int, seed int64) []int64 {
	var xs []int64
	s := int32(seed)
	for k := 0; k < n; k++ {
		// the minimal standard generator; the product stays below 2^47
		s = int32((int64(s) * 48271) % 2147483647)
		if s == 0 {
			s = 1
		}
		xs = append(xs, int64(s))
	}
	return xs
}

func main() {
	var checksum int64 = 0
	passes, _ := strconv.ParseInt(os.Args[1], 10, 64)
	length, _ := strconv.Atoi(os.Args[2])
	for p := int64(0); p < passes; p++ {
		xs := build(length, p+1)
		// stable comparison sort driven by the (a, b) => a - b comparator
		slices.SortStableFunc(xs, func(a, b int64) int {
			d := a - b
			if d < 0 {
				return -1
			}
			if d > 0 {
				return 1
			}
			return 0
		})
		checksum = checksum + xs[0] + xs[length-1]
	}
	fmt.Println(checksum)
}
