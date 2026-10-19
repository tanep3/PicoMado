#include "stream_diag.h"
#if PM_STREAM_DIAGNOSTICS
#include "trace.h"
#include "pico/stdlib.h"
#include <string.h>
// Main owns this collector. The background callback still only copies bytes.
// Samples are taken while the existing SDK lock is already held, at 5 Hz.
static const uint64_t edges[32]={1000,2000,4000,8000,16000,24000,32000,40000,
    50000,60000,70000,80000,90000,100000,120000,150000,180000,220000,
    260000,320000,400000,500000,650000,800000,1000000,1300000,1600000,
    2000000,3000000,5000000,10000000,UINT64_MAX};
typedef struct {uint32_t bins[32];uint64_t count,total,max;uint32_t ge500,ge1000;} histogram;
typedef struct {
    uint64_t us,session,frame,sequence,display_session,display_frame,display_sequence;
    uint32_t rx,app,credit,wnd,nxt,connection;
    uint16_t assembly,flags;
} sample;
#define SAMPLES 24u
typedef struct {
    sample ring[SAMPLES];histogram hist[PM_D_HIST_COUNT];uint32_t counters[PM_D_COUNT];
    uint64_t session,frame,sequence,display_session,display_frame,display_sequence;
    uint64_t begin,end,last_draw,draw_started,frame_started,next_sample;
    uint64_t hook_us,hook_calls,hook_max,trigger_us;
    uint32_t trigger_kind,trigger_value;
    uint32_t head,count,overwritten,assembly,flags;
    bool overflow,have_draw,frame_active,frozen,connected;
} collector;
static collector d;
static uint32_t connection_generation;
static bool connection_valid;
// The SDK lock serializes callback writers with the main-loop drain. The IRQ
// never touches collector/histogram/export state and never prints or allocates.
static volatile uint64_t pending_error_at;
static volatile uint32_t pending_error_value,pending_error_valid,pending_error_count;

static unsigned emit_row;
static uint64_t export_us,export_rows;
_Static_assert(sizeof(collector)<=3072,"bounded unified diagnostics RAM");
static uint32_t add32(uint32_t a,uint32_t b){if(UINT32_MAX-a<b){d.overflow=true;return UINT32_MAX;}return a+b;}
static void hist_add(histogram *h,uint64_t n){
    unsigned bin=0;while(bin<31 && n>=edges[bin])bin++;
    h->bins[bin]=add32(h->bins[bin],1);h->count++;
    if(UINT64_MAX-h->total<n){h->total=UINT64_MAX;d.overflow=true;}else h->total+=n;
    if(n>h->max)h->max=n;
    if(n>=500000)h->ge500=add32(h->ge500,1);
    if(n>=1000000)h->ge1000=add32(h->ge1000,1);
}
static void cost(uint64_t from){
    uint64_t n=time_us_64()-from;d.hook_calls++;d.hook_us+=n;if(n>d.hook_max)d.hook_max=n;
}
size_t pm_diag_ram_bytes(void){return sizeof d+sizeof emit_row+sizeof export_us+sizeof export_rows+sizeof pending_error_at+sizeof pending_error_value+sizeof pending_error_valid+sizeof pending_error_count+sizeof connection_generation+sizeof connection_valid;}
uint64_t pm_diag_hist_count(unsigned h){return h<PM_D_HIST_COUNT?d.hist[h].count:0;}
uint32_t pm_diag_hist_bin(unsigned h,unsigned b){return h<PM_D_HIST_COUNT&&b<32?d.hist[h].bins[b]:0;}
uint32_t pm_diag_counter(pm_diag_kind k){return (unsigned)k<PM_D_COUNT?d.counters[k]:0;}
void pm_diag_event(pm_diag_kind k,uint64_t session,uint64_t frame,uint64_t seq,uint32_t value){
    if(d.frozen || (unsigned)k>=PM_D_COUNT)return;
    uint64_t start=time_us_64();
    d.counters[k]=add32(d.counters[k],1);
    if(!d.trigger_us && d.counters[PM_D_FRAME] && (k==PM_D_CLOSE || k==PM_D_ERROR)){
        d.trigger_us=start;d.trigger_kind=(uint32_t)k;d.trigger_value=value;
    }
    if(k==PM_D_OFFER || k==PM_D_DRAW){
        d.display_session=session;d.display_frame=frame;d.display_sequence=seq;d.flags|=8u;
    }
    if(k==PM_D_OFFER)d.flags|=1u;
    if(k==PM_D_DRAW)d.flags=(d.flags&~1u)|2u;
    if(k==PM_D_RESET || k==PM_D_CLOSE){
        d.flags=0;d.display_session=d.display_frame=d.display_sequence=0;
        pm_diag_abort();
    }
    if(k==PM_D_INVALID)pm_diag_abort();
    if(k==PM_D_CLOSE)connection_valid=false;
    (void)value;
    cost(start);
}
void pm_diag_frame_begin(uint64_t session,uint64_t frame,uint64_t seq){
    if(d.frozen)return;
    pm_diag_event(PM_D_FRAME,session,frame,seq,0);
    d.session=session;d.frame=frame;d.sequence=seq;d.assembly=0;d.flags|=4u;
    d.frame_started=time_us_64();d.frame_active=true;
}
void pm_diag_frame_done(void){
    if(d.frozen || !d.frame_active)return;
    uint64_t start=time_us_64();
    hist_add(&d.hist[PM_D_ASSEMBLY],start-d.frame_started);pm_diag_abort();cost(start);
}
void pm_diag_net_error(uint32_t value){
    if(!pending_error_valid){pending_error_at=time_us_64();pending_error_value=value;pending_error_count=1;pending_error_valid=1;}
    else if(pending_error_count<UINT32_MAX)pending_error_count++;
}
void pm_diag_drain_error(void){
    if(!pending_error_valid)return;
    uint64_t at=pending_error_at;uint32_t value=pending_error_value,n=pending_error_count;pending_error_valid=0;
    if(d.frozen || at<d.begin)return;
    bool already_triggered=d.trigger_us!=0;
    pm_diag_event(PM_D_ERROR,0,0,0,value);
    d.counters[PM_D_ERROR]=add32(d.counters[PM_D_ERROR],n-1);
    if(!already_triggered && d.trigger_us)d.trigger_us=at;
}
void pm_diag_abort(void){
    if(d.frozen)return;
    d.frame_active=false;d.flags&=~4u;d.session=d.frame=d.sequence=0;d.assembly=0;
}
void pm_diag_connection(bool opened){
    connection_valid=opened;
    if(opened){if(connection_generation<UINT32_MAX)connection_generation++;else if(!d.frozen)d.overflow=true;}
}
void pm_diag_progress(uint32_t used){if(!d.frozen && d.frame_active)d.assembly=used;}
void pm_diag_draw_begin(void){if(!d.frozen)d.draw_started=time_us_64();}
void pm_diag_draw_done(uint64_t now){
    if(d.frozen)return;
    uint64_t start=time_us_64();
    if(d.have_draw)hist_add(&d.hist[PM_D_GAP],now-d.last_draw);
    if(d.draw_started)hist_add(&d.hist[PM_D_SPI],now-d.draw_started);
    d.last_draw=now;d.have_draw=true;d.flags&=~2u;cost(start);
}
bool pm_diag_sample_due(uint64_t now){
    return !d.frozen && (!d.trigger_us || now<d.trigger_us+1000000u) && now>=d.next_sample;
}
void pm_diag_sample(uint32_t rx,uint32_t app,uint32_t credit,uint32_t wnd,uint32_t nxt){
    if(d.frozen)return;
    uint64_t start=time_us_64();d.next_sample=start+200000;
    if(d.count==SAMPLES){d.head=(d.head+1)%SAMPLES;d.count--;d.overwritten=add32(d.overwritten,1);}
    sample *s=&d.ring[(d.head+d.count++)%SAMPLES];
    *s=(sample){.us=start,.session=d.session,.frame=d.frame,.sequence=d.sequence,
        .rx=rx,.app=app,.credit=credit,.wnd=wnd,.nxt=nxt,
        .assembly=(uint16_t)d.assembly,.flags=(uint16_t)(d.flags|(connection_valid?16u:0u)),
        .display_session=d.display_session,.display_frame=d.display_frame,.display_sequence=d.display_sequence,.connection=connection_generation};
    d.counters[PM_D_SAMPLE]=add32(d.counters[PM_D_SAMPLE],1);cost(start);
}
void pm_diag_usb(bool connected){
    if(connected==d.connected)return;
    if(connected){d.end=time_us_64();d.frozen=true;d.connected=true;emit_row=0;export_us=export_rows=0;}
    else{memset(&d,0,sizeof d);d.begin=time_us_64();}
}
bool pm_diag_emit(uint32_t epoch){
    if(!d.frozen)return false;
    uint64_t started=time_us_64();unsigned row=emit_row++;
    if(row==2){pm_logf("diag trigger epoch=%lu schema=2 us=%llu k=%lu value=%lu\n",(unsigned long)epoch,
        (unsigned long long)d.trigger_us,(unsigned long)d.trigger_kind,(unsigned long)d.trigger_value);
        export_us+=time_us_64()-started;export_rows++;return true;}
    if(row>2)row--;
    if(row==0)pm_logf("diag begin epoch=%lu start_us=%llu end_us=%llu ram=%u overflow=%u samples=%lu overwritten=%lu\n",
        (unsigned long)epoch,(unsigned long long)d.begin,(unsigned long long)d.end,(unsigned)pm_diag_ram_bytes(),
        d.overflow?1u:0u,(unsigned long)d.count,(unsigned long)d.overwritten);
    else if(row==1)pm_logf("diag hooks epoch=%lu calls=%llu total_us=%llu max_us=%llu\n",(unsigned long)epoch,
        (unsigned long long)d.hook_calls,(unsigned long long)d.hook_us,(unsigned long long)d.hook_max);
    else if(row<=4){
        unsigned h=row-2;histogram *p=&d.hist[h];
        pm_logf("diag hist epoch=%lu h=%u n=%llu sum=%llu max=%llu ge500=%lu ge1000=%lu\n",(unsigned long)epoch,h,
            (unsigned long long)p->count,(unsigned long long)p->total,(unsigned long long)p->max,
            (unsigned long)p->ge500,(unsigned long)p->ge1000);
    }else if(row<=16){
        unsigned h=(row-5)/4,b=((row-5)%4)*8;uint32_t *p=&d.hist[h].bins[b];
        pm_logf("diag bins epoch=%lu h=%u b=%u v=%lu,%lu,%lu,%lu,%lu,%lu,%lu,%lu\n",(unsigned long)epoch,h,b,
            (unsigned long)p[0],(unsigned long)p[1],(unsigned long)p[2],(unsigned long)p[3],
            (unsigned long)p[4],(unsigned long)p[5],(unsigned long)p[6],(unsigned long)p[7]);
    }else if(row<=16+PM_D_COUNT){
        unsigned k=row-17;pm_logf("diag count epoch=%lu k=%u n=%lu\n",(unsigned long)epoch,k,(unsigned long)d.counters[k]);
    }else if(row<=16+PM_D_COUNT+3*d.count){
        unsigned i=(row-17-PM_D_COUNT)/3;sample *s=&d.ring[(d.head+i)%SAMPLES];
        if((row-17-PM_D_COUNT)%3==0)pm_logf("diag time epoch=%lu i=%u us=%llu session=%llu frame=%llu seq=%llu\n",(unsigned long)epoch,i,
            (unsigned long long)s->us,(unsigned long long)s->session,(unsigned long long)s->frame,(unsigned long long)s->sequence);
        else if((row-17-PM_D_COUNT)%3==1)pm_logf("diag draw epoch=%lu i=%u session=%llu frame=%llu seq=%llu tcp=%lu\n",(unsigned long)epoch,i,
            (unsigned long long)s->display_session,(unsigned long long)s->display_frame,(unsigned long long)s->display_sequence,(unsigned long)s->connection);
        else pm_logf("diag buf epoch=%lu i=%u rx=%lu app=%lu credit=%lu wnd=%lu nxt=%lu used=%u flags=%u\n",(unsigned long)epoch,i,
            (unsigned long)s->rx,(unsigned long)s->app,(unsigned long)s->credit,(unsigned long)s->wnd,(unsigned long)s->nxt,
            s->assembly,s->flags);
    }else{pm_logf("diag end epoch=%lu rows=%llu export_us=%llu\n",(unsigned long)epoch,
        (unsigned long long)export_rows,(unsigned long long)export_us);return false;}
    export_us+=time_us_64()-started;export_rows++;return true;
}
#endif
