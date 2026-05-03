package main

import "fmt"

func main() {
	q := make([]int64, 0, 32)
	var total int64 = 0
	var n int64 = 100_000
	for i := int64(0); i < n; i++ {
		q = append(q, i)
		if len(q) > 16 {
			total += q[0]
			q = q[1:]
		}
	}
	for len(q) > 0 {
		total += q[0]
		q = q[1:]
	}
	fmt.Println(total)
}
