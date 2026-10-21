/* Same-core IRQ model, real queue/control/serializer; no physical USB. */
#include <assert.h>
#include <stdarg.h>
#include <stdio.h>
#include <string.h>
#include "usb_control.h"
#include "../src/trace.c"
static unsigned masked,fired,formatted;
static uint64_t inject_at,injected_sequence,clock_us;
static char output[32768];static size_t used;
uint64_t time_us_64(void){return ++clock_us;}
uint32_t save_and_disable_interrupts(void){unsigned prior=masked;masked=1;return prior;}
void restore_interrupts(uint32_t prior){
    masked=prior;
    if(!prior && !fired && inject_at && observation.sequence==inject_at){
        fired=1;
        injected_sequence=pm_observe(PM_OB_NETWORK,PM_OB_NET_RX,PM_OB_RECEIVED,0,0,1,0,10,20);
    }
}
size_t pm_trace_usb_write(const char *p,size_t n){
    assert(!masked && used+n<sizeof output);memcpy(output+used,p,n);used+=n;output[used]=0;return n;
}
static int checked_vsnprintf(char *p,size_t n,const char *format,va_list args){
    assert(!masked);formatted++;return vsnprintf(p,n,format,args);
}
static int checked_snprintf(char *p,size_t n,const char *format,...){
    assert(!masked);va_list args;va_start(args,format);int result=checked_vsnprintf(p,n,format,args);va_end(args);return result;
}
#define snprintf checked_snprintf
#define vsnprintf checked_vsnprintf
#include "../src/usb_control.c"
#undef snprintf
#undef vsnprintf
static void control(const char *p){while(*p)pm_usb_control_input(*p++);pm_usb_control_input('\n');pm_usb_control_poll(true);}
static void drain(void){for(unsigned i=0;i<1000;i++)pm_trace_flush();}
int main(int argc,char **argv){
    assert(argc==2);pm_usb_control_init(0x1234,4);inject_at=1;
    control("EV1 ON 1");drain();
    assert(fired && injected_sequence==2 && observation.sequence==2);
    assert(strstr(output,"ev1 s 1 1234 1 0 0 0 1 0\n"));
    fired=0;inject_at=3;injected_sequence=UINT64_MAX;
    control("EV1 OFF 2");drain();
    assert(fired && !injected_sequence && observation.sequence==3 && !observation.enabled);
    assert(strstr(output,"ev1 s 2 1234 3 0 0 0 0 0\n"));
    FILE *f=fopen(argv[1],"wb");assert(f);assert(fwrite(output,1,used,f)==used);assert(!fclose(f));
    /* Both boundaries can still be lost at the existing observation threshold. */
    inject_at=0;used=0;output[0]=0;pm_observation_enable(true);
    for(unsigned i=0;i<16;i++)pm_observe(PM_OB_NETWORK,PM_OB_NET_RX,PM_OB_RECEIVED,0,0,1,0,0,1);
    assert(count==16 && observation.sequence==19 && !observation.dropped);
    control("EV1 OFF 3");assert(observation.sequence==20 && observation.dropped==1 && !observation.enabled);
    control("EV1 ON 4");assert(observation.sequence==21 && observation.dropped==2 && observation.enabled);
    drain();
    assert(strstr(output,"ev1 s 3 1234 14 1 14 14 0 0\n"));
    assert(strstr(output,"ev1 s 4 1234 15 2 14 15 1 0\n"));
    assert(!masked && formatted);
    puts("PASS atomic ON/OFF: marker/watermark agree, IRQ after ON retained, after OFF suppressed; full queue loss and unmasked formatting preserved");
}
