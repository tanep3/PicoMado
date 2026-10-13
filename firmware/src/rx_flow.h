#pragma once
#include <stdbool.h>
#include <stddef.h>

// TCP data already copied into a bounded buffer may be credited immediately,
// except for the last window-sized reserve. The reserve lets an incomplete
// frame finish arriving even if the application needs the entire frame to parse.
typedef struct {
    size_t used;
    size_t uncredited;
} pm_rx_flow;

static inline void pm_rx_flow_reset(pm_rx_flow *flow) {
    flow->used=0;
    flow->uncredited=0;
}

static inline bool pm_rx_flow_append(pm_rx_flow *flow, size_t n,
                                     size_t capacity, size_t window,
                                     size_t *credit) {
    if(window>capacity || flow->used>capacity || n>capacity-flow->used)return false;
    size_t early_limit=capacity-window;
    size_t available=flow->used<early_limit?early_limit-flow->used:0;
    *credit=n<available?n:available;
    flow->used+=n;
    flow->uncredited+=n-*credit;
    return true;
}

static inline bool pm_rx_flow_consume(pm_rx_flow *flow, size_t n,
                                      size_t *credit) {
    if(n>flow->used)return false;
    *credit=n<flow->uncredited?n:flow->uncredited;
    flow->uncredited-=*credit;
    flow->used-=n;
    return true;
}
