#include "rx_flow.h"
#include <assert.h>
#include <stdint.h>
#include <string.h>

enum { CAP=32768, WINDOW=16*1460, FRAME=17000, COUNT=3 };
static uint8_t wire[COUNT*(FRAME+2)];
static uint8_t received[CAP];
static pm_rx_flow flow;
static size_t budget;

static void reset(void) {
    pm_rx_flow_reset(&flow);
    budget=WINDOW;
    memset(received,0,sizeof received);
}
static void push(const uint8_t *data,size_t n) {
    assert(n<=budget && n<=CAP-flow.used);
    memcpy(received+flow.used,data,n);
    size_t credit=0;
    assert(pm_rx_flow_append(&flow,n,CAP,WINDOW,&credit));
    budget=budget-n+credit;
    assert(flow.used+budget<=CAP && budget<=WINDOW);
}
static bool take_one(unsigned expected) {
    if(flow.used<2)return false;
    size_t length=(size_t)received[0]<<8|received[1];
    assert(length==FRAME);
    if(flow.used<length+2)return false;
    for(size_t i=0;i<length;i++)assert(received[2+i]==expected);
    size_t credit=0;
    assert(pm_rx_flow_consume(&flow,length+2,&credit));
    memmove(received,received+length+2,flow.used);
    budget+=credit;
    assert(flow.used+budget<=CAP && budget<=WINDOW);
    return true;
}
int main(void) {
    for(unsigned f=0;f<COUNT;f++) {
        wire[f*(FRAME+2)]=(uint8_t)(FRAME>>8);
        wire[f*(FRAME+2)+1]=(uint8_t)FRAME;
        memset(wire+f*(FRAME+2)+2,f+1,FRAME);
    }
    reset();
    push(wire,1);assert(!take_one(1));
    push(wire+1,999);assert(!take_one(1));
    size_t sent=1000;
    unsigned consumed=0;
    while(sent<sizeof wire) {
        size_t n=sizeof wire-sent;
        if(n>1460)n=1460;
        if(n>budget)n=budget;
        if(n>CAP-flow.used)n=CAP-flow.used;
        if(n) { push(wire+sent,n);sent+=n; }
        // Simulate a busy main loop: it drains only near the capacity limit.
        // The stream may contain a complete frame plus part of the next.
        if(flow.used>=30000 || !n || sent==sizeof wire) {
            while(consumed<COUNT && take_one(consumed+1))consumed++;
            if(!n)assert(budget>0 || flow.used<2);
        }
    }
    assert(flow.used==0 && flow.uncredited==0);
    reset();
    push(wire,5000);assert(!take_one(1));
    // A dropped socket clears partial bytes and withheld receive credit.
    reset();assert(flow.used==0 && flow.uncredited==0 && budget==WINDOW);
    push(wire,FRAME+2);assert(take_one(1));
    reset();
    assert(!pm_rx_flow_append(&flow,CAP+1,CAP,WINDOW,&budget));
    assert(!pm_rx_flow_append(&flow,1,CAP,CAP+1,&budget));
    assert(!pm_rx_flow_consume(&flow,1,&budget));
    return 0;
}
