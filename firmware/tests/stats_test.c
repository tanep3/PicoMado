#include <assert.h>
#include <stdbool.h>
#include <stdio.h>
#include <string.h>
#include "trace.h"

static uint64_t clock_us=100;
static uint32_t masked,room=256,delay_us,writes;
static bool connected;
static char output[32768];static size_t used;
uint64_t time_us_64(void){return clock_us;}
uint32_t save_and_disable_interrupts(void){uint32_t prior=masked;masked=1;return prior;}
void restore_interrupts(uint32_t prior){masked=prior;}
bool tud_cdc_connected(void){assert(masked);return connected;}
uint32_t tud_cdc_write_available(void){assert(masked);return room;}
uint32_t tud_cdc_write(const void *p,uint32_t n){
    assert(masked && n<=room && used+n<sizeof output);
    memcpy(output+used,p,n);used+=n;output[used]=0;writes++;
    clock_us+=delay_us;delay_us=0;return n;
}
uint32_t tud_cdc_write_flush(void){assert(masked);return 0;}
static void drain_report(void){
    for(unsigned i=0;i<2*PM_STAT_COUNT+4;i++){
        clock_us+=50000;pm_stats_usb_poll(true);pm_trace_flush();
    }
}
int main(void){
    assert(pm_stats_ram_bytes()<=512);
    pm_stat_sample saturated={UINT32_MAX,0};
    pm_stats_accumulate(&saturated,UINT64_MAX);
    assert(saturated.calls==UINT32_MAX && saturated.max_us==UINT32_MAX);
    pm_stats_usb_poll(false);
    for(unsigned i=0;i<PM_STAT_COUNT;i++){
        uint64_t start=clock_us;clock_us+=i+1;pm_stats_note((pm_stat_kind)i,start);
        pm_stat_sample value=pm_stats_read(false,(pm_stat_kind)i);
        assert(value.calls==1 && value.max_us==i+1);
    }
    assert(pm_stats_read(false,PM_STAT_COUNT).calls==0);
    pm_stats_frame_start();clock_us+=3000000;pm_stats_frame_done();pm_stats_frame_done();
    assert(pm_stats_read(false,PM_STAT_FRAME_RX).calls==2);
    assert(pm_stats_read(false,PM_STAT_FRAME_RX).max_us==3000000);
    pm_stats_loop_begin(clock_us);clock_us+=5000;pm_stats_loop_begin(clock_us);
    assert(pm_stats_read(false,PM_STAT_LOOP_GAP).max_us==5000);
    for(unsigned i=0;i<1000;i++)pm_trace_event(PM_TRACE_SPI_END,123,0,0);
    pm_trace_flush();assert(!used && !pm_trace_dropped());
    connected=true;pm_stats_usb_poll(true);
    uint64_t started=clock_us;delay_us=3000000;
    pm_trace_flush();pm_stats_note(PM_STAT_LOOP,started);
    assert(pm_stats_read(true,PM_STAT_USB_WRITE).max_us==3000000);
    assert(pm_stats_read(true,PM_STAT_FLUSH).max_us==3000000);
    assert(pm_stats_read(true,PM_STAT_LOOP).max_us==3000000);
    // The snapshot is immutable: output cost goes into the next USB-on bank.
    drain_report();
    assert(strstr(output,"stats begin epoch=1") && strstr(output,"stats end epoch=1"));
    assert(strstr(output,"usb=off metric=frame_rx calls=2 max_us=3000000"));
    assert(strstr(output,"usb=on metric=usb_write calls=0 max_us=0"));
    size_t before=used;
    for(unsigned i=0;i<100;i++){clock_us+=1000000;pm_stats_usb_poll(true);pm_trace_flush();}
    assert(used==before); // No periodic dump or per-frame output.
    room=0;pm_logf("full endpoint\n");started=clock_us;unsigned sent=writes;
    for(unsigned i=0;i<10000;i++)pm_trace_flush();
    assert(clock_us==started && writes==sent && !masked);
    room=256;pm_trace_flush();
    pm_stats_usb_poll(false);connected=false;
    clock_us=(1ull<<32)+100;
    pm_stats_note(PM_STAT_PROCESS,(1ull<<32)-50);
    assert(pm_stats_read(false,PM_STAT_PROCESS).max_us==150);
    connected=true;pm_stats_usb_poll(true);pm_trace_flush();drain_report();
    assert(strstr(output,"stats end epoch=2"));
    assert(strstr(output,"usb=on metric=usb_write") && strstr(output,"max_us=3000000"));
    // Reopening a short/aborted capture starts a fresh, explicitly numbered report.
    pm_stats_usb_poll(false);connected=false;connected=true;pm_stats_usb_poll(true);
    pm_trace_flush();pm_stats_usb_poll(false);connected=false;
    before=used;clock_us+=100000;pm_stats_usb_poll(false);pm_trace_flush();assert(used==before);
    assert(!masked);
    printf("stats PASS: %zuB RAM, saturation, 64-bit time, frame age, flush cost, USB banks, immutable paced/reopen dumps, quiet/full endpoint\n",pm_stats_ram_bytes());
    return 0;
}
