package main

import (
	"fmt"
	"os"
	"strconv"
)

type Rec struct {
	key     string
	hits    int64
	hasHits bool
	n       int64
}

func main() {
	n, _ := strconv.ParseInt(os.Args[1], 10, 64)
	var recs []Rec
	for i := int64(0); i < 64; i++ {
		key := "k" + strconv.FormatInt(i, 10)
		if i%3 == 0 {
			recs = append(recs, Rec{key: key, n: i})
		} else {
			recs = append(recs, Rec{key: key, hits: i, hasHits: true, n: i})
		}
	}
	// the keys share the records' strings: a lookup allocates nothing
	counts := make(map[string]int64)
	var total int64 = 0
	for i := int64(0); i < n; i++ {
		r := &recs[i&63]
		counts[r.key]++
		hits := int64(1)
		if r.hasHits {
			hits = r.hits
		}
		total = total + hits + r.n
	}
	var sum int64 = 0
	for _, v := range counts {
		sum += v
	}
	fmt.Println(total, sum, len(counts))
}
