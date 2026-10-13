#include "trace.h"
#if PM_TRACE_ENABLE
#include <stdbool.h>
#include <string.h>
#include "pico/stdlib.h"
// Single main-loop producer/consumer. No allocation, waiting, or IRQ printf.
#define RECORDS 32u
#define TEXT_CAP 144u
#define LINE_CAP 192u
#define SLOW_US 50000u
typedef struct {
    uint64_t us;
    unsigned kind;
    union {struct {uint64_t frame;int32_t a;uint32_t b;} event;char text[TEXT_CAP];} data;
} record;
static record queue[RECORDS];
static unsigned head,tail,count;
static uint32_t dropped;
static uint64_t frame_context;
static char pending[LINE_CAP];
static size_t pending_length,pending_offset;
static const char *const names[]={"button","frame_start","crc_ok","receipt_begin","receipt_end",
    "spi_begin","spi_end","render_begin","render_end","tcp_write","tcp_output","slow_poll","slow_loop"};
uint64_t pm_trace_clock(void){return time_us_64();}
uint32_t pm_trace_dropped(void){return dropped;}
void pm_trace_frame_context(uint64_t frame){frame_context=frame;}
uint64_t pm_trace_current_frame(void){return frame_context;}
static record *reserve(void){
    if(count==RECORDS){head=(head+1)%RECORDS;count--;dropped++;}
    record *r=&queue[tail];tail=(tail+1)%RECORDS;count++;
    r->us=pm_trace_clock();return r;
}
void pm_trace_event(pm_trace_kind kind,uint64_t frame,int32_t a,uint32_t b){
    if((unsigned)kind>=sizeof names/sizeof names[0])return;
    record *r=reserve();if(!r)return;
    r->kind=(unsigned)kind;r->data.event.frame=frame;r->data.event.a=a;r->data.event.b=b;
}
void pm_trace_slow(pm_trace_kind kind,uint64_t started,uint32_t where){
    uint64_t elapsed=pm_trace_clock()-started;
    if(elapsed>=SLOW_US)pm_trace_event(kind,0,(int32_t)where,
        elapsed>UINT32_MAX?UINT32_MAX:(uint32_t)elapsed);
}
int pm_logf(const char *format,...){
    record *r=reserve();if(!r)return 0;
    r->kind=UINT32_MAX;
    va_list args;va_start(args,format);
    int n=vsnprintf(r->data.text,sizeof r->data.text,format,args);va_end(args);
    if(n<0){r->data.text[0]=0;return n;}
    if((size_t)n>=sizeof r->data.text){
        r->data.text[TEXT_CAP-2]='\n';r->data.text[TEXT_CAP-1]=0;dropped++;
    }
    return n;
}
void pm_trace_flush(void){
    // At most one bounded record/write per call. Keep partial output; never wait.
    if(!pending_length){
        if(!count)return;
        record *r=&queue[head];
        int n;
        if(r->kind==UINT32_MAX)n=snprintf(pending,sizeof pending,"%s",r->data.text);
        else n=snprintf(pending,sizeof pending,
            "timing us=%llu event=%s frame=%llu a=%ld b=%lu dropped=%lu\n",
            (unsigned long long)r->us,names[r->kind],(unsigned long long)r->data.event.frame,
            (long)r->data.event.a,(unsigned long)r->data.event.b,(unsigned long)dropped);
        head=(head+1)%RECORDS;count--;
        if(n<=0)return;
        pending_length=(size_t)n<sizeof pending?(size_t)n:sizeof pending-1;
        pending_offset=0;
    }
    size_t left=pending_length-pending_offset;
    size_t written=pm_trace_usb_write(pending+pending_offset,left);
    if(written>left)written=left;
    pending_offset+=written;
    if(pending_offset==pending_length){pending_length=0;pending_offset=0;}
}
#endif
