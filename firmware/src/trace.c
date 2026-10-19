#include "trace.h"
#include "rx_stats.h"
#if PM_TRACE_ENABLE
#include <stdbool.h>
#include <string.h>
#include "pico/stdlib.h"
#include "hardware/sync.h"
// Same-core main/IRQ producers, main consumer. No waiting or IRQ printf.
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
void pm_trace_frame_context(uint64_t frame){uint32_t irq=save_and_disable_interrupts();frame_context=frame;restore_interrupts(irq);}
uint64_t pm_trace_current_frame(void){uint32_t irq=save_and_disable_interrupts();uint64_t value=frame_context;restore_interrupts(irq);return value;}
static record *reserve(void){
    if(count==RECORDS){head=(head+1)%RECORDS;count--;dropped++;}
    record *r=&queue[tail];tail=(tail+1)%RECORDS;count++;
    r->us=pm_trace_clock();return r;
}
void pm_trace_event(pm_trace_kind kind,uint64_t frame,int32_t a,uint32_t b){
    if((unsigned)kind>=sizeof names/sizeof names[0])return;
#if !PM_TRACE_VERBOSE
    if(kind!=PM_TRACE_BUTTON)return;
#endif
    uint32_t irq=save_and_disable_interrupts();record *r=reserve();
    r->kind=(unsigned)kind;r->data.event.frame=frame;r->data.event.a=a;r->data.event.b=b;
    restore_interrupts(irq);
}
void pm_trace_slow(pm_trace_kind kind,uint64_t started,uint32_t where){
    uint64_t elapsed=pm_trace_clock()-started;
    if(elapsed>=SLOW_US)pm_trace_event(kind,0,(int32_t)where,
        elapsed>UINT32_MAX?UINT32_MAX:(uint32_t)elapsed);
}
int pm_logf(const char *format,...){
    uint32_t irq=save_and_disable_interrupts();record *r=reserve();
    r->kind=UINT32_MAX;
    va_list args;va_start(args,format);
    int n=vsnprintf(r->data.text,sizeof r->data.text,format,args);va_end(args);
    if(n<0){r->data.text[0]=0;restore_interrupts(irq);return n;}
    if((size_t)n>=sizeof r->data.text){
        r->data.text[TEXT_CAP-2]='\n';r->data.text[TEXT_CAP-1]=0;dropped++;
    }
    restore_interrupts(irq);return n;
}
static void flush_one(void){
    // At most one bounded record/write per call. Keep partial output; never wait.
    if(!pending_length){
        uint32_t irq=save_and_disable_interrupts();
        if(!count){restore_interrupts(irq);return;}
        record saved=queue[head];head=(head+1)%RECORDS;count--;
        uint32_t lost=dropped;restore_interrupts(irq);
        record *r=&saved;
        int n;
        if(r->kind==UINT32_MAX)n=snprintf(pending,sizeof pending,"%s",r->data.text);
        else n=snprintf(pending,sizeof pending,
            "timing us=%llu event=%s frame=%llu a=%ld b=%lu dropped=%lu\n",
            (unsigned long long)r->us,names[r->kind],(unsigned long long)r->data.event.frame,
            (long)r->data.event.a,(unsigned long)r->data.event.b,(unsigned long)lost);
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
void pm_trace_flush(void){
    uint64_t started=pm_stats_clock();
    flush_one();
    pm_stats_note(PM_STAT_FLUSH,started);
}

#if PM_TRACE_STATS
typedef struct {
    pm_stat_sample active[2][PM_STAT_COUNT],snapshot[2][PM_STAT_COUNT];
    uint64_t previous_loop,frame_started,next_emit,window_start,snapshot_start,snapshot_end;
    uint32_t epoch;
    unsigned row;
    bool connected,reporting,frame_active;
#if PM_STREAM_DIAGNOSTICS
    bool rx_done;
#endif
} runtime_stats;
static runtime_stats stats;
_Static_assert(sizeof(runtime_stats)<=1024,"fixed RAM statistics budget");
static const char *const stat_names[]={"loop","loop_gap","sdk_wait","net_poll",
    "rx_take","net_send","process","draw","frame_rx","flush","usb_write","rx_cb"};
size_t pm_stats_ram_bytes(void){return sizeof stats;}
void pm_stats_note(pm_stat_kind kind,uint64_t started){
    if((unsigned)kind>=PM_STAT_COUNT)return;
    uint64_t now=pm_stats_clock();
    uint32_t irq=save_and_disable_interrupts();
    pm_stats_accumulate(&stats.active[stats.connected?1:0][kind],now>=started?now-started:0);
    restore_interrupts(irq);
}
pm_stat_sample pm_stats_read(bool connected,pm_stat_kind kind){
    pm_stat_sample value={0};
    if((unsigned)kind>=PM_STAT_COUNT)return value;
    uint32_t irq=save_and_disable_interrupts();value=stats.active[connected?1:0][kind];restore_interrupts(irq);
    return value;
}
void pm_stats_loop_begin(uint64_t started){
    if(stats.previous_loop)pm_stats_note(PM_STAT_LOOP_GAP,stats.previous_loop);
    stats.previous_loop=started;
}
void pm_stats_frame_start(void){stats.frame_started=pm_stats_clock();stats.frame_active=true;}
void pm_stats_frame_done(void){
    if(stats.frame_active)pm_stats_note(PM_STAT_FRAME_RX,stats.frame_started);
    stats.frame_active=false;
}
void pm_stats_usb_poll(bool connected){
    uint64_t now=pm_stats_clock();
    pm_rx_usb(connected);
    pm_diag_usb(connected);
    uint32_t irq=save_and_disable_interrupts();
    bool rising=connected&&!stats.connected;
    stats.connected=connected;
    if(rising){
        memcpy(stats.snapshot,stats.active,sizeof stats.active);
        memset(stats.active,0,sizeof stats.active);
        stats.snapshot_start=stats.window_start;stats.snapshot_end=now;stats.window_start=now;
        stats.epoch++;stats.row=0;stats.next_emit=now;stats.reporting=true;pm_rx_capture(now);
#if PM_STREAM_DIAGNOSTICS
        stats.rx_done=false;
#endif
    }
    if(!connected)stats.reporting=false;
    restore_interrupts(irq);
    if(!stats.reporting || now<stats.next_emit)return;
    stats.next_emit=now+20000u;
    unsigned row=stats.row++;
    if(row==0){
        pm_logf("stats begin epoch=%lu start_us=%llu end_us=%llu metrics=%u banks=2 rx_schema=2\n",
            (unsigned long)stats.epoch,(unsigned long long)stats.snapshot_start,
            (unsigned long long)stats.snapshot_end,(unsigned)PM_STAT_COUNT);
    }else if(row<=2*PM_STAT_COUNT){
        unsigned bank=(row-1)/PM_STAT_COUNT,kind=(row-1)%PM_STAT_COUNT;
        pm_stat_sample value=stats.snapshot[bank][kind];
#if PM_STREAM_DIAGNOSTICS
        pm_logf("diag metric epoch=%lu usb=%s metric=%s total_us=%llu\n",(unsigned long)stats.epoch,bank?"on":"off",stat_names[kind],(unsigned long long)value.total_us);
#endif
        pm_logf("stats epoch=%lu usb=%s metric=%s calls=%lu max_us=%lu\n",
            (unsigned long)stats.epoch,bank?"on":"off",stat_names[kind],
            (unsigned long)value.calls,(unsigned long)value.max_us);
    }else if(row==2*PM_STAT_COUNT+1){
        pm_logf("stats end epoch=%lu trace_dropped=%lu\n",(unsigned long)stats.epoch,(unsigned long)pm_trace_dropped());
#if PM_STREAM_DIAGNOSTICS
    }else if(!stats.rx_done){if(!pm_rx_emit(stats.epoch))stats.rx_done=true;
    }else if(!pm_diag_emit(stats.epoch)){stats.reporting=false;}
#else
    }else if(!pm_rx_emit(stats.epoch)){stats.reporting=false;}
#endif
}
#endif
#endif
