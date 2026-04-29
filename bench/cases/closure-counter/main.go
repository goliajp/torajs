package main

import "fmt"

func loopSum(xs []int64, f func(int64) int64) int64 {
	var sum int64 = 0
	for _, x := range xs {
		sum = sum + f(x)
	}
	return sum
}

func main() {
	var xs []int64
	for i := int64(0); i < 10000000; i++ {
		xs = append(xs, i)
	}
	var offset int64 = 2
	add := func(x int64) int64 { return x + offset }
	fmt.Println(loopSum(xs, add))
}
