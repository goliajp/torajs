enum State {
    Start,
    AfterYield,
    Done,
}

// function* range(start, end) as its generator state machine; the object
// lives on the heap like a js generator object
struct Range {
    state: State,
    start: i64,
    end: i64,
    i: i64,
}

impl Iterator for Range {
    type Item = i64;
    fn next(&mut self) -> Option<i64> {
        match self.state {
            State::Start => self.i = self.start,
            State::AfterYield => self.i = self.i + 1,
            State::Done => return None,
        }
        if self.i < self.end {
            self.state = State::AfterYield;
            Some(self.i)
        } else {
            self.state = State::Done;
            None
        }
    }
}

fn range(start: i64, end: i64) -> Box<Range> {
    Box::new(Range {
        state: State::Start,
        start,
        end,
        i: 0,
    })
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut total: i64 = 0;
    let passes: i32 = args.next().unwrap().parse().unwrap();
    let end: i64 = args.next().unwrap().parse().unwrap();
    for _ in 0..passes {
        for v in range(0, end) {
            total = total + v;
        }
    }
    println!("{}", total);
}
