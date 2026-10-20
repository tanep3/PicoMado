#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "usb_control.h"
// Seed the real private counters at their boundary; do not execute 2^64 events.
#include "../src/trace.c"
static char output[512];static size_t used;
uint64_t time_us_64(void){return UINT64_MAX;}
uint32_t save_and_disable_interrupts(void){return 0;}
void restore_interrupts(uint32_t irq){assert(!irq);}
size_t pm_trace_usb_write(const char *p,size_t n){assert(used+n<sizeof output);memcpy(output+used,p,n);used+=n;output[used]=0;return n;}
int main(void){
 pm_observation_init(UINT64_MAX);pm_observation_enable(true);
 observation.sequence=UINT64_MAX-1;
 assert(pm_observe(PM_OB_INPUT,1,PM_OB_RECEIVED,0,0,1,0,0,0)==UINT64_MAX);
 assert(!pm_observe(PM_OB_INPUT,1,PM_OB_RECEIVED,0,0,1,0,0,0));
 pm_observation_status s=pm_observation_read();assert(s.sequence==UINT64_MAX && s.saturated && !s.enabled);
 observation.sequence=UINT64_MAX-2;observation.dropped=UINT64_MAX;observation.enabled=true;count=RECORDS/2;
 assert(pm_observe(PM_OB_INPUT,1,PM_OB_RECEIVED,0,0,1,0,0,0)==UINT64_MAX-1);
 assert(observation.dropped==UINT64_MAX && observation.saturated);
 head=tail=count=0;pending_length=pending_offset=0;
 pm_usb_control_init(UINT64_MAX,0);
 observation.sequence=observation.dropped=observation.first_lost=observation.last_lost=UINT64_MAX;
 const char *command="EV1 STATUS ffffffffffffffff\n";
 while(*command)pm_usb_control_input(*command++);
 pm_usb_control_poll(true);for(unsigned i=0;i<10;i++)pm_trace_flush();
 assert(strstr(output,"ev1 s ffffffffffffffff ffffffffffffffff ffffffffffffffff ffffffffffffffff ffffffffffffffff ffffffffffffffff 1 1\n"));
 assert(used<144);
 puts("PASS real EV1 u64 saturation/no-wrap and maximum tagged control reply below144B");
 return 0;
}
