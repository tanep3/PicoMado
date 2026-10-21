#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "usb_control.h"
#include "../src/trace.c"
static uint64_t clock_us;
static size_t room=64,output_bytes,calls;
static uint32_t cost=3,masked;
static char output[1024*1024];
uint64_t time_us_64(void){return clock_us;}
uint32_t save_and_disable_interrupts(void){uint32_t before=masked;masked=1;return before;}
void restore_interrupts(uint32_t before){masked=before;}
size_t pm_trace_usb_write(const char *p,size_t n){
    assert(!masked);calls++;clock_us+=cost;if(n>room)n=room;
    assert(output_bytes+n<sizeof output);memcpy(output+output_bytes,p,n);output_bytes+=n;return n;
}
static void event(unsigned component,unsigned phase){pm_observe(component,1,phase,0,0,1,2,0,0);}
static void idle(void){for(unsigned i=0;i<2000 && (count || pending_length);i++)pm_observation_drain(4,768,500);assert(!count && !pending_length);}
static void command(const char *p){while(*p)pm_usb_control_input(*p++);pm_usb_control_input('\n');pm_usb_control_poll(true);}
int main(void){
    pm_usb_control_init(123,4);pm_observation_enable(true);
    for(unsigned frame=0;frame<100;frame++){
        event(PM_OB_NETWORK,PM_OB_RECEIVED); /* callback containing a burst */
        event(PM_OB_NETWORK,PM_OB_COMPLETED);event(PM_OB_NETWORK,PM_OB_DISPATCHED);
        event(PM_OB_API,PM_OB_RECEIVED);event(PM_OB_API,PM_OB_DISPATCHED);
        event(PM_OB_FRAME,PM_OB_RECEIVED);event(PM_OB_API,PM_OB_PENDING);
        pm_observation_drain(6,768,200);
        for(unsigned chunk=0;chunk<6;chunk++){
            event(PM_OB_NETWORK,PM_OB_COMPLETED);event(PM_OB_NETWORK,PM_OB_DISPATCHED);
            if(chunk==5)event(PM_OB_FRAME,PM_OB_COMPLETED);
            pm_observation_drain(6,768,200);
        }
        event(PM_OB_DISPLAY,PM_OB_DISPATCHED);clock_us+=19000;event(PM_OB_DISPLAY,PM_OB_COMPLETED);
        pm_observation_drain(4,768,500);clock_us+=60000;
        for(unsigned i=0;i<8;i++)pm_observation_drain(4,768,500);
    }
    idle();assert(observation.sequence==2200 && !observation.dropped);
    uint64_t offered=metrics.offered,sequence=observation.sequence;
    size_t written=output_bytes,old_calls=calls;
    for(unsigned i=0;i<1000;i++){pm_observation_metrics m=pm_observation_metrics_read();assert(m.offered==offered);pm_observation_drain(8,1024,500);}
    assert(!count && !pending_length && calls==old_calls && output_bytes==written && observation.sequence==sequence);
    room=0;event(PM_OB_API,PM_OB_RECEIVED);old_calls=calls;
    pm_observation_drain(8,1024,500);assert(calls==old_calls+1 && metrics.zero_writes==1);
    room=1;old_calls=calls;size_t before=output_bytes;
    pm_observation_drain(1000,1000000,1000000);assert(calls-old_calls<=8 && output_bytes-before<=8);
    room=64;cost=250;old_calls=calls;pm_observation_drain(8,1024,200);assert(calls==old_calls+1);
    cost=3;idle();assert(!count && !pending_length);
    sequence=observation.sequence;command("OBS METRICS ffffffffffffffff 0");idle();
    output[output_bytes]=0;assert(strstr(output,"obs m ffffffffffffffff 0 "));
    assert(observation.sequence==sequence); /* No metrics recursion. */
    pm_observation_metrics m=pm_observation_metrics_read();assert(m.offered==2201 && m.partial_writes && m.budget_stops && m.max_drain_us>=250);
    metrics.offered=UINT64_MAX;metrics.bytes=UINT64_MAX-1;
    event(PM_OB_API,PM_OB_RECEIVED);idle();
    m=pm_observation_metrics_read();assert(m.saturated&&m.offered==UINT64_MAX&&m.bytes==UINT64_MAX);
    puts("PASS bounded cold drain, 12fps burst/no loss, zero/partial USB, no recursive metrics and idle queue empty");
}
