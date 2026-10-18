#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "rx_stats.h"
static uint64_t clock_us=100;
static uint32_t masked;
static char output[32768];static size_t used;static unsigned lines;
uint64_t time_us_64(void){return clock_us;}
uint64_t pm_trace_clock(void){return clock_us;}
uint32_t save_and_disable_interrupts(void){uint32_t old=masked;masked=1;return old;}
void restore_interrupts(uint32_t old){masked=old;}
int pm_logf(const char *format,...){
    assert(!masked);char line[512];va_list args;va_start(args,format);
    int n=vsnprintf(line,sizeof line,format,args);va_end(args);
    assert(n>0 && n<144 && used+(size_t)n<sizeof output);
    memcpy(output+used,line,(size_t)n);used+=(size_t)n;output[used]=0;lines++;return n;
}
static void emit(void){unsigned before=lines;for(unsigned i=0;i<120;i++)if(!pm_rx_emit(17))break;assert(lines-before<=102);}
static pm_rx_record record(unsigned slot){pm_rx_record r;assert(pm_rx_snapshot_record(slot,&r));return r;}
int main(int argc,char **argv){
    pm_rx_state s={.tcp_valid=1,.wnd=23360,.ann=23360,.nxt=1000,
        .alloc={.flags=7,.bits=16,.pool_err=65535,.heap_used=100}};
    assert(pm_rx_ram_bytes()<=4096);pm_rx_connection(&s);pm_rx_usb(false);
    // Low-buffer arrival gap: the foreground can keep polling without RX data.
    pm_rx_frame_begin(101,7,&s);clock_us+=10;
    pm_rx_callback_begin(1460,&s);s.rx=1460;s.alloc.pool_used=1;
    pm_rx_callback_end(1,1460,&s);clock_us+=10;s.rx=0;s.alloc.pool_used=0;
    pm_rx_consume(1460,0,&s);pm_rx_take(false,&s);
    uint64_t gap_start=clock_us-10;clock_us+=3000000;
    assert(pm_rx_sample_due());pm_rx_sample(&s);assert(!pm_rx_sample_due());
    pm_rx_callback_begin(1460,&s);pm_rx_callback_end(1,1460,&s);
    pm_rx_progress(25600);pm_rx_finish(PM_RX_COMPLETE,0,&s);pm_rx_capture(clock_us);
    pm_rx_record r=record(0);assert(r.frame==101 && r.session==7 && r.connection==1);
    assert(r.gap_from==gap_start && r.gap_to==clock_us && r.gap_to-r.gap_from==3000010);
    assert(!r.gap_rx1 && r.gap_rx0==1460 && r.rx_peak==1460 && r.callbacks==2);
    assert(r.bytes==2920 && r.freed==2 && r.free_calls==2 && r.early_bytes==2920);
    assert(r.assembled==25600 && r.reason==PM_RX_COMPLETE && !r.failure_mask);
    // Later active updates must not mutate the frozen record/output.
    clock_us+=10;s.rx=32000;s.credit=23360;s.app=4300;
    pm_rx_frame_begin(201,7,&s);clock_us+=3000000;pm_rx_sample(&s);
    assert(record(0).frame==101);emit();assert(strstr(output,"frame=101 session=7"));
    pm_rx_finish(PM_RX_COMPLETE,0,&s);pm_rx_capture(clock_us);r=record(0);
    assert(r.frame==201 && r.rx_peak==32000 && r.credit_peak==23360 && r.app_peak==4300);
    assert(r.gap_anchor==1 && r.gap_tail==1 && r.gap_rx0==0 && r.gap_rx1==32000 && r.gap_credit1==23360);
    // No callbacks: periodic sampling records a wrapped pool failure before an abort.
    s.rx=s.app=s.credit=0;clock_us+=100;pm_rx_connection(&s);pm_rx_frame_begin(202,8,&s);
    clock_us+=50000;s.alloc.pool_err=0;s.alloc.pool_used=24;s.alloc.heap_err=1;s.alloc.tcp_drop=1;
    pm_rx_sample(&s);uint64_t first_failure=clock_us;
    clock_us+=3000000;pm_rx_fault(PM_RX_TCP_ERROR,-14,&s);pm_rx_capture(clock_us);
    r=record(1);assert(r.frame==202 && r.reason==PM_RX_TCP_ERROR && (int32_t)r.error==-14);
    assert(r.gap_to-r.gap_from==3050000 && !r.callbacks && r.pool_peak==24);
    assert(r.before.alloc.pool_err==65535 && r.after.alloc.pool_err==0);
    assert(r.failure_mask==11 && r.first_failure==first_failure);
    // Superseded, active-at-capture, USB straddle, and tail gaps have explicit IDs.
    clock_us=(1ull<<32)+100;pm_rx_connection(&s);pm_rx_frame_begin(UINT64_MAX,UINT64_MAX,&s);
    clock_us+=2000000;pm_rx_frame_begin(302,9,&s);clock_us+=4000000;
    pm_rx_usb(true);pm_rx_capture(clock_us);r=record(1);
    assert(r.frame==UINT64_MAX && r.reason==PM_RX_REPLACED && r.end-r.start==2000000);
    r=record(3);assert(r.frame==302 && r.reason==PM_RX_ACTIVE && r.usb==3);
    assert(r.gap_to==clock_us && r.gap_to-r.gap_from==4000000);
    emit();assert(strstr(output,"frame=18446744073709551615 session=18446744073709551615"));
    clock_us+=1000000;pm_rx_finish(PM_RX_COMPLETE,0,&s);pm_rx_capture(clock_us);
    r=record(0);assert(r.frame==302 && r.end-r.start==5000000 && r.usb==3);
    // Availability flags prevent unsupported counters from becoming false failures.
    s.alloc.flags=s.alloc.bits=0;clock_us+=100;pm_rx_frame_begin(401,9,&s);
    s.alloc.pool_err=500;s.alloc.heap_err=500;clock_us+=10;pm_rx_sample(&s);
    pm_rx_callback_begin(UINT32_MAX,&s);pm_rx_callback_end(0,UINT32_MAX,&s);
    pm_rx_callback_begin(1,&s);pm_rx_callback_end(1,1,&s);
    pm_rx_finish(PM_RX_COMPLETE,0,&s);pm_rx_capture(clock_us);r=record(0);
    assert(!r.failure_mask && !r.before.alloc.flags && !r.after.alloc.bits);
    assert(r.bytes==UINT32_MAX && r.early_bytes==UINT32_MAX && r.freed==1);
    // Four most recent connection faults, bounded even without a frame.
    for(unsigned i=0;i<9;i++){clock_us++;pm_rx_connection(&s);pm_rx_fault(PM_RX_PEER_CLOSED,0,&s);}
    pm_rx_capture(clock_us);used=lines=0;output[0]=0;emit();
    assert(strstr(output,"events=4 parts=22 event_parts=3"));assert(lines<=102);
    if(argc==2){FILE *file=fopen(argv[1],"w");assert(file);fputs(output,file);assert(!fclose(file));}
    assert(!masked);
    printf("rx stats PASS: %zuB fixed RAM, arrival/consumer gaps, frame IDs, tail/abort/replacement, 64-bit time, USB straddle, immutable snapshots, allocation wrap/availability, bounded events/lines\n",pm_rx_ram_bytes());
}
