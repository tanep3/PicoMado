#include "rx_stats.h"
#if PM_TRACE_STATS
#include <string.h>
#include "hardware/sync.h"
typedef struct {
    uint64_t at,frame;
    uint32_t connection,reason,error,rx,app,credit,pool_err,heap_err,flags;
} rx_event;
typedef struct {
    pm_rx_record current,complete,aborted,idle,snapshot[4];
    pm_rx_state latest,callback_state;
    rx_event events[4],event_snapshot[4];
    uint64_t next_sample,captured,last_callback;
    uint32_t connection,event_count,event_next,snapshot_events,emit_row;
    bool active,tracking,connected;
} receive_stats;
static receive_stats rx;
_Static_assert(sizeof(receive_stats)<=4096,"fixed receive diagnostics RAM budget");
static uint32_t plus(uint32_t a,uint32_t b){return UINT32_MAX-a<b?UINT32_MAX:a+b;}
static uint64_t elapsed(uint64_t a,uint64_t b){return b>=a?b-a:0;}
static void sample_record(pm_rx_record *r,const pm_rx_state *s,uint64_t now){
    r->after=*s;
    if(s->rx>r->rx_peak){r->rx_peak=s->rx;r->rx_peak_at=now;}
    if(s->app>r->app_peak)r->app_peak=s->app;
    if(s->credit>r->credit_peak){r->credit_peak=s->credit;r->credit_peak_at=now;}
    if(s->tcp_valid){
        if(s->wnd<r->wnd_min)r->wnd_min=s->wnd;
        if(s->ann<r->ann_min)r->ann_min=s->ann;
        r->ooseq_seen|=s->ooseq;
    }
    if(s->alloc.pool_used>r->pool_peak)r->pool_peak=s->alloc.pool_used;
    if(s->alloc.heap_used>r->heap_peak)r->heap_peak=s->alloc.heap_used;
    if(s->alloc.seg_used>r->seg_peak)r->seg_peak=s->alloc.seg_used;
    const pm_rx_alloc *a=&r->before.alloc,*b=&s->alloc;
    uint32_t mask=0;
    if((a->flags&b->flags&PM_RX_POOL) && a->pool_err!=b->pool_err)mask|=1;
    if((a->flags&b->flags&PM_RX_HEAP) && a->heap_err!=b->heap_err)mask|=2;
    if((a->flags&b->flags&PM_RX_SEG) && a->seg_err!=b->seg_err)mask|=64;
    if(a->flags&b->flags&PM_RX_TCP){
        if(a->tcp_memerr!=b->tcp_memerr)mask|=4;
        if(a->tcp_drop!=b->tcp_drop)mask|=8;
        if(a->tcp_chkerr!=b->tcp_chkerr)mask|=16;
        if(a->tcp_err!=b->tcp_err)mask|=32;
    }
    if(mask && !r->failure_mask)r->first_failure=now;
    r->failure_mask|=mask;
}
static void gap(pm_rx_record *r,const pm_rx_state *end,uint64_t now,bool tail){
    uint64_t from=r->last_cb?r->last_cb:r->start;
    if(elapsed(from,now)>elapsed(r->gap_from,r->gap_to)){
        r->gap_from=from;r->gap_to=now;r->gap_anchor=r->last_cb!=0;r->gap_tail=tail;
        r->gap_rx0=rx.callback_state.rx;r->gap_credit0=rx.callback_state.credit;
        r->gap_rx1=end->rx;r->gap_credit1=end->credit;
    }
}
static void finish_locked(pm_rx_reason reason,int error,const pm_rx_state *state,uint64_t now){
    if(!rx.tracking)return;
    pm_rx_record *r=&rx.current;
    sample_record(r,state,now);gap(r,state,now,true);r->end=now;
    r->reason=rx.active?(uint32_t)reason:PM_RX_IDLE;r->error=(uint32_t)error;
    r->usb|=rx.connected?2u:1u;
    pm_rx_record *saved=!rx.active?&rx.idle:reason==PM_RX_COMPLETE?&rx.complete:&rx.aborted;
    if(!saved->reason || elapsed(saved->start,saved->end)<=elapsed(r->start,r->end))*saved=*r;
    rx.active=rx.tracking=false;
}
static void begin_locked(uint64_t frame,uint64_t session,const pm_rx_state *state,uint64_t now){
    memset(&rx.current,0,sizeof rx.current);
    pm_rx_record *r=&rx.current;r->frame=frame;r->session=session;
    r->connection=rx.connection;r->start=now;r->last_cb=rx.last_callback;
    r->reason=frame?PM_RX_ACTIVE:PM_RX_IDLE;r->before=*state;
    r->wnd_min=r->ann_min=UINT32_MAX;r->usb=rx.connected?2u:1u;
    rx.active=frame!=0;rx.tracking=true;rx.latest=*state;rx.next_sample=now+50000u;
    if(!rx.last_callback)rx.callback_state=*state;
    sample_record(r,state,now);
}
size_t pm_rx_ram_bytes(void){return sizeof rx;}
void pm_rx_sample(const pm_rx_state *state){
    uint64_t now=pm_stats_clock();uint32_t irq=save_and_disable_interrupts();
    rx.latest=*state;
    if(rx.tracking){sample_record(&rx.current,state,now);rx.current.usb|=rx.connected?2u:1u;}
    restore_interrupts(irq);
}
bool pm_rx_sample_due(void){
    uint64_t now=pm_stats_clock();uint32_t irq=save_and_disable_interrupts();
    bool due=rx.tracking && now>=rx.next_sample;
    if(due)rx.next_sample=now+50000u;
    restore_interrupts(irq);return due;
}
void pm_rx_frame_begin(uint64_t frame,uint64_t session,const pm_rx_state *state){
    uint64_t now=pm_stats_clock();uint32_t irq=save_and_disable_interrupts();
    finish_locked(PM_RX_REPLACED,0,state,now);
    begin_locked(frame,session,state,now);restore_interrupts(irq);
}
void pm_rx_finish(pm_rx_reason reason,int error,const pm_rx_state *state){
    uint64_t now=pm_stats_clock();uint32_t irq=save_and_disable_interrupts();
    rx.latest=*state;
    if(rx.tracking){uint64_t session=rx.current.session;finish_locked(reason,error,state,now);begin_locked(0,session,state,now);}
    restore_interrupts(irq);
}
void pm_rx_callback_begin(uint32_t bytes,const pm_rx_state *state){
    uint64_t now=pm_stats_clock();uint32_t irq=save_and_disable_interrupts();
    rx.latest=*state;
    if(rx.tracking){
        pm_rx_record *r=&rx.current;gap(r,state,now,false);r->last_cb=now;
        r->callbacks=plus(r->callbacks,1);r->bytes=plus(r->bytes,bytes);
        sample_record(r,state,now);
    }
    rx.last_callback=now;restore_interrupts(irq);
}
void pm_rx_callback_end(uint32_t freed,uint32_t credit,const pm_rx_state *state){
    uint64_t now=pm_stats_clock();uint32_t irq=save_and_disable_interrupts();
    rx.latest=rx.callback_state=*state;
    if(rx.tracking){
        pm_rx_record *r=&rx.current;r->free_calls=plus(r->free_calls,1);r->freed=plus(r->freed,freed);
        if(credit){r->early_calls=plus(r->early_calls,1);r->early_bytes=plus(r->early_bytes,credit);}
        sample_record(r,state,now);
    }
    restore_interrupts(irq);
}
void pm_rx_consume(uint32_t bytes,uint32_t credit,const pm_rx_state *state){
    uint64_t now=pm_stats_clock();uint32_t irq=save_and_disable_interrupts();
    rx.latest=*state;
    if(rx.tracking){
        pm_rx_record *r=&rx.current;r->consume_calls=plus(r->consume_calls,1);
        r->consumed=plus(r->consumed,bytes);r->last_consume=now;
        if(credit)r->consume_credit=plus(r->consume_credit,credit);
        sample_record(r,state,now);
    }
    restore_interrupts(irq);
}
void pm_rx_take(bool message,const pm_rx_state *state){
    uint64_t now=pm_stats_clock();uint32_t irq=save_and_disable_interrupts();
    rx.latest=*state;
    if(rx.tracking){
        pm_rx_record *r=&rx.current;r->takes=plus(r->takes,1);
        if(message)r->messages=plus(r->messages,1);
        sample_record(r,state,now);
    }
    restore_interrupts(irq);
}
void pm_rx_progress(uint32_t bytes){
    uint64_t now=pm_stats_clock();uint32_t irq=save_and_disable_interrupts();
    if(rx.active){rx.current.assembled=bytes;rx.current.progress_at=now;}
    restore_interrupts(irq);
}
static void event_locked(pm_rx_reason reason,int error,const pm_rx_state *state,uint64_t now){
    rx_event *e=&rx.events[rx.event_next];
    *e=(rx_event){.at=now,.frame=rx.active?rx.current.frame:0,.connection=rx.connection,
        .reason=(uint32_t)reason,.error=(uint32_t)error,.rx=state->rx,.app=state->app,
        .credit=state->credit,.pool_err=state->alloc.pool_err,.heap_err=state->alloc.heap_err,.flags=state->alloc.flags};
    rx.event_next=(rx.event_next+1)%4;if(rx.event_count<4)rx.event_count++;
}
void pm_rx_connection(const pm_rx_state *state){
    uint64_t now=pm_stats_clock();uint32_t irq=save_and_disable_interrupts();
    finish_locked(PM_RX_RESET,0,state,now);
    rx.connection=plus(rx.connection,1);event_locked(PM_RX_CONNECT,0,state,now);rx.latest=*state;
    rx.last_callback=0;rx.callback_state=*state;begin_locked(0,0,state,now);
    restore_interrupts(irq);
}
void pm_rx_closed(const pm_rx_state *state){
    uint64_t now=pm_stats_clock();uint32_t irq=save_and_disable_interrupts();
    finish_locked(PM_RX_RESET,0,state,now);rx.latest=*state;rx.last_callback=0;
    restore_interrupts(irq);
}
void pm_rx_fault(pm_rx_reason reason,int error,const pm_rx_state *state){
    uint64_t now=pm_stats_clock();uint32_t irq=save_and_disable_interrupts();
    event_locked(reason,error,state,now);finish_locked(reason,error,state,now);rx.latest=*state;
    restore_interrupts(irq);
}
void pm_rx_usb(bool connected){
    uint32_t irq=save_and_disable_interrupts();rx.connected=connected;
    if(rx.tracking)rx.current.usb|=connected?2u:1u;
    restore_interrupts(irq);
}
void pm_rx_capture(uint64_t at){
    uint32_t irq=save_and_disable_interrupts();
    rx.snapshot[0]=rx.complete;rx.snapshot[1]=rx.aborted;rx.snapshot[2]=rx.idle;
    memset(&rx.snapshot[3],0,sizeof rx.snapshot[3]);
    if(rx.tracking){
        rx.snapshot[3]=rx.current;pm_rx_record *r=&rx.snapshot[3];
        sample_record(r,&rx.latest,at);gap(r,&rx.latest,at,true);r->end=at;
    }
    memset(&rx.complete,0,sizeof rx.complete);memset(&rx.aborted,0,sizeof rx.aborted);memset(&rx.idle,0,sizeof rx.idle);
    rx.snapshot_events=rx.event_count;
    for(unsigned i=0;i<rx.event_count;i++)rx.event_snapshot[i]=rx.events[(rx.event_next+4-rx.event_count+i)%4];
    rx.event_count=rx.event_next=0;rx.captured=at;rx.emit_row=0;
    restore_interrupts(irq);
}
bool pm_rx_snapshot_record(unsigned slot,pm_rx_record *out){
    if(slot>=4)return false;
    uint32_t irq=save_and_disable_interrupts();*out=rx.snapshot[slot];restore_interrupts(irq);
    return out->reason!=0;
}
// Fixed schema, <=143 bytes per line even at maximum integer widths.
static void emit_record(uint32_t epoch,unsigned slot,unsigned part,const pm_rx_record *r){
#define LINE(name,fmt,...) pm_logf("rx epoch=%lu slot=%u part=" name " " fmt "\n",(unsigned long)epoch,slot,__VA_ARGS__)
#define U(v) ((unsigned long)(v))
#define T(v) ((unsigned long long)(v))
    switch(part){
    case 0:LINE("id","frame=%llu session=%llu conn=%lu reason=%lu usb=%lu",T(r->frame),T(r->session),U(r->connection),U(r->reason),U(r->usb));break;
    case 1:LINE("time","start_us=%llu end_us=%llu",T(r->start),T(r->end));break;
    case 2:LINE("gap","from_us=%llu to_us=%llu",T(r->gap_from),T(r->gap_to));break;
    case 3:LINE("gap0","rx=%lu credit=%lu anchor=%lu tail=%lu",U(r->gap_rx0),U(r->gap_credit0),U(r->gap_anchor),U(r->gap_tail));break;
    case 4:LINE("gap1","rx=%lu credit=%lu",U(r->gap_rx1),U(r->gap_credit1));break;
    case 5:LINE("progress","bytes=%lu at_us=%llu last_cb_us=%llu",U(r->assembled),T(r->progress_at),T(r->last_cb));break;
    case 6:LINE("peaks","rx=%lu app=%lu credit=%lu pool=%lu heap=%lu",U(r->rx_peak),U(r->app_peak),U(r->credit_peak),U(r->pool_peak),U(r->heap_peak));break;
    case 7:LINE("peak_time","rx_us=%llu credit_us=%llu",T(r->rx_peak_at),T(r->credit_peak_at));break;
    case 8:LINE("calls","callbacks=%lu bytes=%lu free_calls=%lu freed=%lu",U(r->callbacks),U(r->bytes),U(r->free_calls),U(r->freed));break;
    case 9:LINE("early","calls=%lu bytes=%lu",U(r->early_calls),U(r->early_bytes));break;
    case 10:LINE("consume","calls=%lu credit=%lu bytes=%lu last_us=%llu",U(r->consume_calls),U(r->consume_credit),U(r->consumed),T(r->last_consume));break;
    case 11:LINE("take","calls=%lu messages=%lu",U(r->takes),U(r->messages));break;
    case 12:LINE("end_buf","rx=%lu app=%lu credit=%lu",U(r->after.rx),U(r->after.app),U(r->after.credit));break;
    case 13:LINE("end_tcp","valid=%lu wnd=%lu ann=%lu nxt=%lu ooseq=%lu",U(r->after.tcp_valid),U(r->after.wnd),U(r->after.ann),U(r->after.nxt),U(r->after.ooseq));break;
    case 14:LINE("wnd","min=%lu ann_min=%lu ooseq_seen=%lu",U(r->wnd_min),U(r->ann_min),U(r->ooseq_seen));break;
    case 15:LINE("alloc_b0","flags=%lu bits=%lu pool_err=%lu heap_err=%lu",U(r->before.alloc.flags),U(r->before.alloc.bits),U(r->before.alloc.pool_err),U(r->before.alloc.heap_err));break;
    case 16:LINE("alloc_b1","tcp_mem=%lu drop=%lu chk=%lu err=%lu",U(r->before.alloc.tcp_memerr),U(r->before.alloc.tcp_drop),U(r->before.alloc.tcp_chkerr),U(r->before.alloc.tcp_err));break;
    case 17:LINE("alloc_e0","flags=%lu bits=%lu pool_err=%lu heap_err=%lu",U(r->after.alloc.flags),U(r->after.alloc.bits),U(r->after.alloc.pool_err),U(r->after.alloc.heap_err));break;
    case 18:LINE("alloc_e1","tcp_mem=%lu drop=%lu chk=%lu err=%lu",U(r->after.alloc.tcp_memerr),U(r->after.alloc.tcp_drop),U(r->after.alloc.tcp_chkerr),U(r->after.alloc.tcp_err));break;
    case 19:LINE("alloc_used","pool0=%lu heap0=%lu pool1=%lu heap1=%lu",U(r->before.alloc.pool_used),U(r->before.alloc.heap_used),U(r->after.alloc.pool_used),U(r->after.alloc.heap_used));break;
    case 20:LINE("failure","first_us=%llu mask=%lu error=%ld",T(r->first_failure),U(r->failure_mask),(long)(int32_t)r->error);break;
    case 21:LINE("seg","err0=%lu err1=%lu used0=%lu used1=%lu peak=%lu",U(r->before.alloc.seg_err),U(r->after.alloc.seg_err),U(r->before.alloc.seg_used),U(r->after.alloc.seg_used),U(r->seg_peak));break;
    }
#undef LINE
}
bool pm_rx_emit(uint32_t epoch){
    unsigned row=rx.emit_row++;
    if(!row){
        unsigned records=0;for(unsigned i=0;i<4;i++)if(rx.snapshot[i].reason)records++;
        pm_logf("rx begin epoch=%lu schema=2 records=%u events=%lu parts=%u event_parts=%u at_us=%llu\n",
            U(epoch),records,U(rx.snapshot_events),PM_RX_RECORD_PARTS,PM_RX_EVENT_PARTS,T(rx.captured));return true;
    }
    row--;
    for(unsigned i=0;i<4;i++)if(rx.snapshot[i].reason){
        if(row<PM_RX_RECORD_PARTS){emit_record(epoch,i,row,&rx.snapshot[i]);return true;}
        row-=PM_RX_RECORD_PARTS;
    }
    if(row<rx.snapshot_events*PM_RX_EVENT_PARTS){
        unsigned i=row/PM_RX_EVENT_PARTS,part=row%PM_RX_EVENT_PARTS;const rx_event *e=&rx.event_snapshot[i];
        if(part==0)pm_logf("rxevent epoch=%lu slot=%u part=id frame=%llu conn=%lu reason=%lu error=%ld\n",U(epoch),i,T(e->frame),U(e->connection),U(e->reason),(long)(int32_t)e->error);
        else if(part==1)pm_logf("rxevent epoch=%lu slot=%u part=time at_us=%llu rx=%lu app=%lu credit=%lu\n",U(epoch),i,T(e->at),U(e->rx),U(e->app),U(e->credit));
        else pm_logf("rxevent epoch=%lu slot=%u part=alloc flags=%lu pool_err=%lu heap_err=%lu\n",U(epoch),i,U(e->flags),U(e->pool_err),U(e->heap_err));
        return true;
    }
    pm_logf("rx end epoch=%lu\n",U(epoch));return false;
}
#undef U
#undef T
#endif
