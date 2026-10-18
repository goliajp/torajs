#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

typedef void (*JobFn)(void *env, int64_t v);

typedef struct {
    JobFn fn;
    void *env;
    int64_t v;
} Job;

/* the fifo microtask queue, a power-of-two ring */
static Job *jobs;
static size_t jobs_cap, jobs_head, jobs_len;

static void enqueue(JobFn fn, void *env, int64_t v) {
    if (jobs_len == jobs_cap) {
        size_t cap = jobs_cap == 0 ? 64 : jobs_cap * 2;
        Job *next = malloc(cap * sizeof(Job));
        for (size_t k = 0; k < jobs_len; k++) {
            next[k] = jobs[(jobs_head + k) & (jobs_cap - 1)];
        }
        free(jobs);
        jobs = next;
        jobs_cap = cap;
        jobs_head = 0;
    }
    jobs[(jobs_head + jobs_len) & (jobs_cap - 1)] = (Job){fn, env, v};
    jobs_len++;
}

static void run_microtasks(void) {
    while (jobs_len > 0) {
        Job j = jobs[jobs_head];
        jobs_head = (jobs_head + 1) & (jobs_cap - 1);
        jobs_len--;
        j.fn(j.env, j.v);
    }
}

typedef struct Reaction {
    JobFn fn;
    void *env;
    struct Reaction *next;
} Reaction;

typedef struct {
    uint32_t rc;
    int fulfilled;
    int64_t value;
    Reaction *head, *tail;
} Promise;

static Promise *promise_new(void) {
    Promise *p = malloc(sizeof *p);
    *p = (Promise){1, 0, 0, NULL, NULL};
    return p;
}

static void promise_release(Promise *p) {
    if (--p->rc == 0) {
        free(p);
    }
}

static void promise_resolve(Promise *p, int64_t v) {
    p->fulfilled = 1;
    p->value = v;
    Reaction *r = p->head;
    p->head = p->tail = NULL;
    while (r) {
        Reaction *next = r->next;
        enqueue(r->fn, r->env, v);
        free(r);
        r = next;
    }
}

/* a settled promise queues the job now; a pending one keeps it in order */
static void promise_react(Promise *p, JobFn fn, void *env) {
    if (p->fulfilled) {
        enqueue(fn, env, p->value);
        return;
    }
    Reaction *r = malloc(sizeof *r);
    *r = (Reaction){fn, env, NULL};
    if (p->tail) {
        p->tail->next = r;
    } else {
        p->head = r;
    }
    p->tail = r;
}

/* async function step(i) { await null; return i * 3 + 1 } as its frame and
   state machine: state 0 queues its own resumption and suspends, state 1
   settles the result promise and finishes */
typedef struct {
    int state;
    int64_t i;
    Promise *ret;
} StepFrame;

static void step_step(void *env, int64_t v) {
    (void)v;
    StepFrame *f = env;
    if (f->state == 0) {
        f->state = 1;
        enqueue(step_step, f, 0);
        return;
    }
    promise_resolve(f->ret, f->i * 3 + 1);
    promise_release(f->ret);
    free(f);
}

static Promise *step_call(int64_t i) {
    StepFrame *f = malloc(sizeof *f);
    Promise *ret = promise_new();
    ret->rc++;
    *f = (StepFrame){0, i, ret};
    step_step(f, 0);
    return ret;
}

/* the module body as an async state machine; state 1 resumes after the await */
typedef struct {
    int state;
    int64_t i;
    int64_t total;
    Promise *awaiting;
} MainFrame;

static int64_t iterations;

static void main_step(void *env, int64_t v) {
    MainFrame *f = env;
    switch (f->state) {
    case 0:
        f->total = 0;
        f->i = 0;
        break;
    case 1:
        promise_release(f->awaiting);
        f->total = f->total + v;
        f->i = f->i + 1;
        break;
    }
    if (f->i < iterations) {
        f->awaiting = step_call(f->i);
        f->state = 1;
        promise_react(f->awaiting, main_step, f);
        return;
    }
    printf("%" PRId64 "\n", f->total);
    free(f);
}

int main(int argc, char **argv) {
    (void)argc;
    iterations = strtoll(argv[1], NULL, 10);
    MainFrame *f = malloc(sizeof *f);
    f->state = 0;
    main_step(f, 0);
    run_microtasks();
    free(jobs);
    return 0;
}
