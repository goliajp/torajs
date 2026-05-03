package main

import "fmt"

func main() {
	xs := make([]int64, 0, 1_000_000)
	var n int64 = 1_000_000
	for i := int64(0); i < n; i++ {
		xs = append(xs, i)
	}
	var total int64 = 0
	for len(xs) > 0 {
		v := xs[len(xs)-1]
		xs = xs[:len(xs)-1]
		total += v
	}
	fmt.Println(total)
}
