#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "trace.h"
#include "core.h"
static uint64_t clock_us;
static bool connected;
static uint32_t room,irq_depth,writes,flushes;
static char output[32768];static size_t used;
uint64_t time_us_64(void){return clock_us;}
uint32_t save_and_disable_interrupts(void){assert(!irq_depth);irq_depth++;return 17;}
void restore_interrupts(uint32_t saved){assert(saved==17 && irq_depth==1);irq_depth--;}
bool tud_cdc_connected(void){assert(irq_depth==1);return connected;}
uint32_t tud_cdc_write_available(void){assert(irq_depth==1);return room;}
uint32_t tud_cdc_write(const void *p,uint32_t n){
    assert(irq_depth==1 && n && n<=room && used+n<sizeof output);
    memcpy(output+used,p,n);used+=n;output[used]=0;writes++;return n;
}
uint32_t tud_cdc_write_flush(void){assert(irq_depth==1);flushes++;return 0;}
int main(void){
    pm_trace_event(PM_TRACE_CRC_OK,255,0,25600);
    unsigned cycles=10000,button_polls=0,network_polls=0;
    pm_button wake={0};bool fired=false;
    for(unsigned i=0;i<cycles;i++){
        clock_us+=5000;
        if(pm_button_wake_step(&wake,4,2,true,(uint32_t)(clock_us/1000),1500))fired=true;
        button_polls++;network_polls++;
        pm_trace_event(PM_TRACE_BUTTON,0,2,5);pm_trace_flush();
        assert(!irq_depth && !writes && !flushes);
    }
    assert(fired && button_polls==cycles && network_polls==cycles && pm_trace_dropped()>0);
    connected=true;room=0;uint64_t before=clock_us;
    for(unsigned i=0;i<10000;i++)pm_trace_flush();
    assert(clock_us==before && !writes && !flushes && !irq_depth);
    room=7;for(unsigned i=0;i<5000;i++)pm_trace_flush();
    assert(strstr(output,"event=crc_ok frame=255") && strstr(output,"event=button"));
    assert(writes==flushes && !irq_depth);
    used=0;output[0]=0;room=256;
    clock_us=1234567;pm_trace_event(PM_TRACE_SPI_BEGIN,258,0,0);
    clock_us+=14000;pm_trace_event(PM_TRACE_SPI_END,258,1,0);
    pm_trace_slow(PM_TRACE_SLOW_POLL,clock_us-49999,2);
    pm_trace_slow(PM_TRACE_SLOW_POLL,clock_us-50000,1);
    pm_logf("net status dummy\n");
    for(unsigned i=0;i<100;i++)pm_trace_flush();
    assert(strstr(output,"us=1234567 event=spi_begin frame=258"));
    assert(strstr(output,"us=1248567 event=spi_end frame=258"));
    assert(strstr(output,"event=slow_poll frame=0 a=1 b=50000"));
    assert(!strstr(output,"a=2 b=49999"));assert(strstr(output,"net status dummy\n"));
    puts("trace PASS: absent/full/partial USB; bounded writes; 10000 button/network cycles; overflow counted; original monotonic event times");
}
