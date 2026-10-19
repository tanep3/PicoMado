#include "stream_diag.h"
#include <assert.h>
#include <stdio.h>
#include <stdarg.h>
#include <string.h>
static uint64_t clock_us=1;
uint64_t time_us_64(void){return clock_us++;}
static char output[32768];static size_t output_used,max_line;
int pm_logf(const char *format,...){
    char line[256];va_list a;va_start(a,format);int n=vsnprintf(line,sizeof line,format,a);va_end(a);
    assert(n>0&&(size_t)n<144);if((size_t)n>max_line)max_line=(size_t)n;
    assert(output_used+(size_t)n<sizeof output);memcpy(output+output_used,line,(size_t)n);output_used+=(size_t)n;output[output_used]=0;return n;
}
// Access pure collection internals for boundary/saturation tests; SDK IO is mocked.
#include "../src/stream_diag.c"
static void draw(uint64_t interval){
    clock_us+=interval;pm_diag_draw_begin();clock_us+=39000;pm_diag_draw_done(clock_us);
}
int main(int argc,char **argv){
    for(unsigned i=0;i<10000;i++){
        pm_diag_event(PM_D_OFFER,1,i+1,i,0);draw(44000);
    }
    assert(pm_diag_hist_count(PM_D_GAP)==9999&&pm_diag_hist_count(PM_D_SPI)==10000);
    uint64_t sum=0;for(unsigned i=0;i<32;i++)sum+=pm_diag_hist_bin(PM_D_GAP,i);assert(sum==9999);
    assert(d.hist[PM_D_GAP].ge500==0&&d.hist[PM_D_GAP].ge1000==0);
    draw(500000);draw(1000000);
    assert(d.hist[PM_D_GAP].ge500==2&&d.hist[PM_D_GAP].ge1000==1);
    pm_diag_event(PM_D_RESET,0,0,0,0);draw(1500000);
    assert(d.hist[PM_D_GAP].ge500==3&&d.hist[PM_D_GAP].ge1000==2);
    puts("PASS all_draw_intervals_including_reconnect_gap_no_record_high_bias");
    pm_diag_frame_begin(2,20000,10);clock_us+=2300000;pm_diag_progress(25600);pm_diag_frame_done();
    pm_diag_frame_done();assert(pm_diag_hist_count(PM_D_ASSEMBLY)==1&&d.hist[PM_D_ASSEMBLY].ge1000==1);
    puts("PASS completed_assembly_count_once_partial_frames_not_counted");
    for(unsigned i=0;i<100;i++){
        clock_us+=200000;assert(pm_diag_sample_due(clock_us));pm_diag_sample(i,100,0,23360,i);
        assert(!pm_diag_sample_due(clock_us));
    }
    assert(d.count==24&&d.overwritten==76&&d.ring[d.head].rx==76);
    assert(pm_diag_counter(PM_D_SAMPLE)==100&&d.hook_calls>10000&&d.hook_us>0);
    puts("PASS bounded_ring_overwrite_count_and_measured_hook_cost");
    uint32_t old_errors=pm_diag_counter(PM_D_ERROR);pm_diag_net_error(14);pm_diag_net_error(99);
    assert(pm_diag_counter(PM_D_ERROR)==old_errors&&!d.trigger_us);
    pm_diag_drain_error();assert(pm_diag_counter(PM_D_ERROR)==old_errors+2);
    assert(d.trigger_us&&d.trigger_value==14);
    uint64_t first_trigger=d.trigger_us;clock_us+=20;pm_diag_net_error(3);pm_diag_drain_error();assert(d.trigger_us==first_trigger);
    clock_us=d.trigger_us+1000001;assert(!pm_diag_sample_due(clock_us));
    uint64_t before_trigger=pm_diag_hist_count(PM_D_GAP);draw(50000);
    assert(pm_diag_hist_count(PM_D_GAP)==before_trigger+1);
    puts("PASS first_failure_retains_ring_while_full_window_histograms_continue");
    pm_diag_usb(true);uint64_t n=pm_diag_hist_count(PM_D_GAP);
    for(unsigned i=0;i<100;i++){draw(100000);pm_diag_event(PM_D_BUSY,0,0,0,0);}
    assert(pm_diag_hist_count(PM_D_GAP)==n&&pm_diag_counter(PM_D_BUSY)==0);
    unsigned lines=0;while(pm_diag_emit(1)){assert(++lines<200);}
    assert(strstr(output,"diag trigger epoch=1")&&strstr(output,"diag begin epoch=1")&&strstr(output,"diag end epoch=1"));
    assert(strstr(output,"ge500=4 ge1000=3")&&strstr(output,"samples=24 overwritten=76"));
    printf("PASS immutable_USB_off_snapshot: ram=%zu rows=%u max_line=%zu\n",pm_diag_ram_bytes(),lines,max_line);
    if(argc>1){FILE *f=fopen(argv[1],"w");assert(f);assert(fwrite(output,1,output_used,f)==output_used);fclose(f);}
    pm_diag_usb(false);assert(pm_diag_hist_count(PM_D_GAP)==0&&d.count==0);
    histogram *h=&d.hist[0];h->bins[0]=UINT32_MAX;hist_add(h,0);assert(h->bins[0]==UINT32_MAX&&d.overflow);
    memset(h,0,sizeof *h);hist_add(h,499999);hist_add(h,500000);hist_add(h,999999);hist_add(h,1000000);
    assert(h->ge500==3&&h->ge1000==1&&h->count==4);
    puts("PASS histogram_exact_thresholds_saturation_flag_and_new_window_reset");
}
