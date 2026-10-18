// suppressed: the rust row blocks vectorization with black_box per element; gc does not vectorize this loop, so the plain loop does the same scalar work
package main

import (
	"fmt"
	"os"
	"strconv"
)

func main() {
	count, _ := strconv.ParseInt(os.Args[1], 10, 64)
	xs := make([]int64, 0)
	for i := int64(0); i < count; i++ {
		if i%2 == 0 {
			xs = append(xs, i)
		} else {
			xs = append(xs, -i)
		}
	}
	var sum int64 = 0
	for j := 0; j < len(xs); j++ {
		sum = sum + xs[j]
	}
	fmt.Println(sum)
}
