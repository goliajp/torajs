package main

import "fmt"

//go:noinline
func loopSum(n int64, k int64) int64 {
	xs := make([]int64, 0, n)
	for i := int64(0); i < n; i++ {
		xs = append(xs, i)
	}
	add := func(x int64) int64 { return x + k }
	ys := make([]int64, 0, len(xs))
	for _, x := range xs {
		ys = append(ys, add(x))
	}
	var sum int64 = 0
	for _, y := range ys {
		sum = sum + y
	}
	return sum
}

func main() {
	fmt.Println(loopSum(10000000, 2))
}
