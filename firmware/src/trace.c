#include "trace.h"
#include "rx_stats.h"
#include "observation.h"
#if PM_EVENT_STREAM && !PM_TRACE_ENABLE
#error "Event stream requires the bounded trace backend"
#endif
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
    union {struct {uint64_t frame;int32_t a;uint32_t b;} event;char text[TEXT_CAP];
#if PM_EVENT_STREAM
        struct {uint64_t sequence,operation,parent,session,frame,dropped;uint32_t a,b;uint16_t event;uint8_t component,phase;} observation;
#endif
    } data;
} record;
static record queue[RECORDS];
static unsigned head,tail,count;
static uint32_t dropped;
static uint64_t frame_context;
#if PM_EVENT_STREAM
#define OBSERVATION_KIND (UINT32_MAX-1u)
static pm_observation_status observation;
static pm_observation_metrics metrics;
static bool observation_negotiated;
static void metric_add(uint64_t *counter,uint64_t amount){
    if(UINT64_MAX-*counter<amount){*counter=UINT64_MAX;metrics.saturated=true;}
    else *counter+=amount;
}
static void metric_queue(void){if(count>metrics.highwater)metrics.highwater=count;}
pm_observation_metrics pm_observation_metrics_read(void){
    uint32_t irq=save_and_disable_interrupts();pm_observation_metrics m=metrics;
    restore_interrupts(irq);return m;
}
static void observation_lost(uint64_t seq){
    if(!observation.first_lost)observation.first_lost=seq;
    observation.last_lost=seq;
    if(observation.dropped<UINT64_MAX)observation.dropped++;else observation.saturated=true;
}
void pm_observation_init(uint64_t boot){observation.boot=boot?boot:1;}
void pm_observation_enable(bool enabled){
    uint32_t irq=save_and_disable_interrupts();
    if(enabled && !observation.enabled)observation.capture=observation.sequence==UINT64_MAX?UINT64_MAX:observation.sequence+1;
    observation.enabled=enabled;
    if(enabled)observation_negotiated=true;
    restore_interrupts(irq);
}
pm_observation_status pm_observation_read(void){
    uint32_t irq=save_and_disable_interrupts();pm_observation_status s=observation;s.queued=count;
    restore_interrupts(irq);return s;
}
uint64_t pm_observe(uint8_t component,uint16_t event,uint8_t phase,
    uint64_t operation,uint64_t parent,uint64_t session,uint64_t frame,uint32_t a,uint32_t b){
    uint32_t irq=save_and_disable_interrupts();
    if(!observation.enabled){restore_interrupts(irq);return 0;}
    metric_add(&metrics.offered,1);
    if(observation.sequence==UINT64_MAX){observation.saturated=true;observation.enabled=false;restore_interrupts(irq);return 0;}
    uint64_t seq=++observation.sequence;
    /* Half the shared queue remains available to command replies. Drop newest;
     * never wait, overwrite a control reply, format or enter the SDK here. */
    if(count>=RECORDS/2){observation_lost(seq);restore_interrupts(irq);return seq;}
    record *r=&queue[tail];tail=(tail+1)%RECORDS;count++;metric_queue();
    r->kind=OBSERVATION_KIND;r->us=time_us_64();
    r->data.observation.sequence=seq;r->data.observation.operation=operation?operation:seq;
    r->data.observation.parent=parent;r->data.observation.session=session;r->data.observation.frame=frame;
    r->data.observation.a=a;r->data.observation.b=b;r->data.observation.component=component;
    r->data.observation.event=event;r->data.observation.phase=phase;r->data.observation.dropped=observation.dropped;
    restore_interrupts(irq);return seq;
}
static void put_be(uint8_t **p,uint64_t value,unsigned bytes){
    for(unsigned i=bytes;i;i--)*(*p)++=(uint8_t)(value>>(8*(i-1)));
}
static int observation_line(const record *r,char *out){
    uint8_t body[76],*p=body;const char *hex="0123456789abcdef";
    put_be(&p,r->data.observation.component,1);put_be(&p,r->data.observation.event,2);put_be(&p,r->data.observation.phase,1);
    put_be(&p,observation.boot,8);put_be(&p,r->data.observation.sequence,8);put_be(&p,r->us,8);
    put_be(&p,r->data.observation.operation,8);put_be(&p,r->data.observation.parent,8);
    put_be(&p,r->data.observation.session,8);put_be(&p,r->data.observation.frame,8);
    put_be(&p,r->data.observation.dropped,8);put_be(&p,r->data.observation.a,4);put_be(&p,r->data.observation.b,4);
    uint32_t crc=UINT32_MAX;memcpy(out,"ev1 ",4);unsigned n=4;
    for(unsigned i=0;i<sizeof body;i++){
        crc^=body[i];for(unsigned j=0;j<8;j++)crc=(crc>>1)^((crc&1)?UINT32_C(0xedb88320):0);
        out[n++]=hex[body[i]>>4];out[n++]=hex[body[i]&15];
    }
    crc=~crc;out[n++]=' ';
    for(unsigned i=8;i;i--)out[n++]=hex[(crc>>(4*(i-1)))&15];
    out[n++]='\n';out[n]=0;return (int)n;
}
#endif
static char pending[LINE_CAP];
static size_t pending_length,pending_offset;
static const char *const names[]={"button","frame_start","crc_ok","receipt_begin","receipt_end",
    "spi_begin","spi_end","render_begin","render_end","tcp_write","tcp_output","slow_poll","slow_loop"};
uint64_t pm_trace_clock(void){return time_us_64();}
uint32_t pm_trace_dropped(void){return dropped;}
void pm_trace_frame_context(uint64_t frame){uint32_t irq=save_and_disable_interrupts();frame_context=frame;restore_interrupts(irq);}
uint64_t pm_trace_current_frame(void){uint32_t irq=save_and_disable_interrupts();uint64_t value=frame_context;restore_interrupts(irq);return value;}
static record *reserve(void){
    #if PM_EVENT_STREAM
    if(observation_negotiated && count>=RECORDS/2){if(dropped<UINT32_MAX)dropped++;return NULL;}
#endif
    if(count==RECORDS){head=(head+1)%RECORDS;count--;dropped++;}
    record *r=&queue[tail];tail=(tail+1)%RECORDS;count++;
#if PM_EVENT_STREAM
    if(observation_negotiated)metric_queue();
#endif
    r->us=pm_trace_clock();return r;
}
void pm_trace_event(pm_trace_kind kind,uint64_t frame,int32_t a,uint32_t b){
    if((unsigned)kind>=sizeof names/sizeof names[0])return;
#if !PM_TRACE_VERBOSE
    if(kind!=PM_TRACE_BUTTON)return;
#endif
    uint32_t irq=save_and_disable_interrupts();record *r=reserve();
    if(!r){restore_interrupts(irq);return;}
    r->kind=(unsigned)kind;r->data.event.frame=frame;r->data.event.a=a;r->data.event.b=b;
    restore_interrupts(irq);
}
void pm_trace_slow(pm_trace_kind kind,uint64_t started,uint32_t where){
    uint64_t elapsed=pm_trace_clock()-started;
    if(elapsed>=SLOW_US)pm_trace_event(kind,0,(int32_t)where,
        elapsed>UINT32_MAX?UINT32_MAX:(uint32_t)elapsed);
}
int pm_logf(const char *format,...){
    /* Formatting is outside the IRQ mask; producers only copy a bounded value. */
    char text[TEXT_CAP];va_list args;va_start(args,format);
    int n=vsnprintf(text,sizeof text,format,args);va_end(args);
    if(n<0)return n;
    if((size_t)n>=sizeof text){text[TEXT_CAP-2]='\n';text[TEXT_CAP-1]=0;}
    uint32_t irq=save_and_disable_interrupts();record *r=reserve();
    if(r){r->kind=UINT32_MAX;memcpy(r->data.text,text,(size_t)n<sizeof text?(size_t)n+1:sizeof text);}
    if((size_t)n>=sizeof text && dropped<UINT32_MAX)dropped++;
    restore_interrupts(irq);return n;
}
#if PM_ACK_PATH_PROBE || PM_USB_BUTTON_SIM || PM_EVENT_STREAM
// Frozen probe export shares the ordinary line queue: partial USB writes from
// different producers must never interleave within a line. Never evict here.
bool pm_trace_try_line(const char *line,size_t length){
    if(length>=TEXT_CAP)return false;
    uint32_t irq=save_and_disable_interrupts();
    unsigned limit=RECORDS/2;
#if PM_EVENT_STREAM
    if(observation_negotiated)limit=RECORDS;
#endif
    if(count>=limit){restore_interrupts(irq);return false;}
    record *r=&queue[tail];tail=(tail+1)%RECORDS;count++;r->us=pm_trace_clock();r->kind=UINT32_MAX;memcpy(r->data.text,line,length);r->data.text[length]=0;
#if PM_EVENT_STREAM
    if(observation_negotiated)metric_queue();
#endif
    restore_interrupts(irq);return true;
}
#endif
static size_t flush_one(size_t cap){
    // At most one bounded record/write per call. Keep partial output; never wait.
    if(!pending_length){
        uint32_t irq=save_and_disable_interrupts();
        if(!count){restore_interrupts(irq);return 0;}
        record saved=queue[head];head=(head+1)%RECORDS;count--;
        uint32_t lost=dropped;restore_interrupts(irq);
        record *r=&saved;
        int n;
        #if PM_EVENT_STREAM
        if(r->kind==OBSERVATION_KIND)n=observation_line(r,pending);
        else
#endif
        if(r->kind==UINT32_MAX)n=snprintf(pending,sizeof pending,"%s",r->data.text);
        else n=snprintf(pending,sizeof pending,
            "timing us=%llu event=%s frame=%llu a=%ld b=%lu dropped=%lu\n",
            (unsigned long long)r->us,names[r->kind],(unsigned long long)r->data.event.frame,
            (long)r->data.event.a,(unsigned long)r->data.event.b,(unsigned long)lost);
        if(n<=0)return 0;
        pending_length=(size_t)n<sizeof pending?(size_t)n:sizeof pending-1;
        pending_offset=0;
    }
    size_t left=pending_length-pending_offset;
    if(left>cap)left=cap;
    if(!left)return 0;
    size_t written=pm_trace_usb_write(pending+pending_offset,left);
    if(written>left)written=left;
    pending_offset+=written;
#if PM_EVENT_STREAM
    if(observation_negotiated){
        metric_add(&metrics.write_calls,1);metric_add(&metrics.bytes,written);
        if(!written)metric_add(&metrics.zero_writes,1);
        else if(pending_offset<pending_length)metric_add(&metrics.partial_writes,1);
    }
#endif
    if(pending_offset==pending_length){pending_length=0;pending_offset=0;}
    return written;
}
void pm_trace_flush(void){
    uint64_t started=pm_stats_clock();
    (void)flush_one(SIZE_MAX);
    pm_stats_note(PM_STAT_FLUSH,started);
}
#if PM_EVENT_STREAM
void pm_observation_drain(unsigned calls,size_t bytes,uint32_t microseconds){
    if(!observation.enabled || !calls || !bytes || !microseconds)return;
    if(calls>8)calls=8;
    if(bytes>1024)bytes=1024;
    if(microseconds>500)microseconds=500;
    metric_add(&metrics.drain_calls,1);uint64_t started=time_us_64();
    unsigned used=0;size_t written=0;
    while(used<calls && written<bytes && (pending_length || count)){
        if(time_us_64()-started>=microseconds)break;
        size_t n=flush_one(bytes-written);used++;written+=n;
        if(!n)break;
    }
    uint64_t elapsed=time_us_64()-started;
    if(elapsed>metrics.max_drain_us)metrics.max_drain_us=elapsed;
    if((pending_length || count) && (used==calls || written==bytes || elapsed>=microseconds))metric_add(&metrics.budget_stops,1);
}
#endif

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
