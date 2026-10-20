#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "trace.h"
#include "usb_control.h"
static uint64_t now=UINT64_C(1)<<40;
static unsigned masked,writes,room;
static char output[32768];static size_t used;
uint64_t time_us_64(void){return now;}
uint32_t save_and_disable_interrupts(void){unsigned prior=masked;masked=1;return prior;}
void restore_interrupts(uint32_t prior){masked=prior;}
size_t pm_trace_usb_write(const char *p,size_t n){
    assert(!masked);writes++;if(n>room)n=room;assert(used+n<sizeof output);
    memcpy(output+used,p,n);used+=n;output[used]=0;return n;
}
static void command(const char *s){while(*s)pm_usb_control_input(*s++);pm_usb_control_input('\n');pm_usb_control_poll(true);}
static void drain(void){room=7;for(unsigned i=0;i<5000;i++)pm_trace_flush();}
int main(int argc,char **argv){
    assert(argc==2);pm_usb_control_init(0x1234,4);
    assert(!pm_observe(PM_OB_API,12,PM_OB_RECEIVED,0,0,1,2,3,4));
    command("EV1 ON");drain();used=0;output[0]=0;
    for(unsigned i=0;i<1000;i++)pm_observe(PM_OB_API,12,PM_OB_RECEIVED,0,0,1,2,3,i);
    pm_observation_status s=pm_observation_read();
    assert(s.sequence==1001 && s.dropped==984 && s.first_lost==18 && s.last_lost==1001 && s.queued==16);
    command("EV1 STATUS");room=0;
    for(unsigned i=0;i<10000;i++)pm_trace_flush();
    assert(!used && !masked);drain();
    assert(strstr(output,"ev1 status b=1234 seq=3e9 drop=3d8 first=12 last=3e9 on=1 sat=0\n"));
    assert(pm_observe(PM_OB_DISPLAY,1,PM_OB_COMPLETED,1,0,1,2,1,0)==1002);drain();
    command("EV1 OFF");drain();
    assert(!pm_observe(PM_OB_API,12,PM_OB_RECEIVED,0,0,1,2,3,4));
    command("EV1 ON");drain();
    assert(pm_observe(PM_OB_API,12,PM_OB_COMPLETED,1,0,1,2,0,0)==1005);drain();
    command("EV1 STATUS ffffffffffffffff");drain();
    assert(strstr(output,"ev1 s ffffffffffffffff 1234 3ed 3d8 12 3e9 1 0\n"));
    unsigned events=0;
    for(char *p=output;*p;){char *end=strchr(p,'\n');assert(end);
        if(!strncmp(p,"ev1 ",4) && p[4]!='s'){assert(end-p==165);events++;}
        else {assert(!strncmp(p,"ev1 status ",11) || !strncmp(p,"ev1 s ",6));assert(end-p<144);}
        p=end+1;
    }
    assert(events==20 && !masked && writes>10000);
    FILE *f=fopen(argv[1],"wb");assert(f);assert(fwrite(output,1,used,f)==used);assert(!fclose(f));
    puts("PASS bounded shared event/control queue, 984 explicit drops, 64-bit time, 7-byte writes, stop/resume watermark");
}
