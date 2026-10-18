// Full production snapshot/queue/USB path; output must be finite and immutable.
#define main original_stats_test
#include "stats_test.c"
#undef main
#include "rx_stats.h"
int main(int argc,char **argv){
    pm_rx_state s={.tcp_valid=1,.wnd=23360,.ann=23360,.alloc={.flags=15,.bits=16}};
    for(unsigned i=0;i<4;i++){pm_rx_connection(&s);clock_us+=100;}
    pm_rx_frame_begin(900,7,&s);clock_us+=3000000;pm_rx_finish(PM_RX_COMPLETE,0,&s);
    pm_rx_frame_begin(901,7,&s);clock_us+=2000000;pm_rx_finish(PM_RX_INVALID,0,&s);
    clock_us+=1000000;pm_rx_frame_begin(902,7,&s);clock_us+=500000;
    connected=true;uint64_t begin=clock_us;pm_stats_usb_poll(true);
    assert(pm_rx_ram_bytes()<=4096);unsigned report_end_ms=0;
    // Changes after capture are live data for the next window, not this dump.
    clock_us++;pm_rx_frame_begin(999,8,&s);s.rx=1000;pm_rx_sample(&s);
    room=7;
    for(unsigned i=0;i<5000;i++){
        clock_us+=1000;pm_stats_usb_poll(true);pm_trace_flush();
        if(!report_end_ms && strstr(output,"rx end epoch=1\n"))report_end_ms=(unsigned)((clock_us-begin)/1000);
    }
    assert(report_end_ms && report_end_ms<3500 && !pm_trace_dropped());
    assert(strstr(output,"records=4 events=4 parts=22 event_parts=3"));
    assert(strstr(output,"frame=900 session=7") && strstr(output,"frame=901 session=7") && strstr(output,"frame=902 session=7"));
    assert(!strstr(output,"frame=999") && strstr(output,"stats end epoch=1"));
    size_t before=used;for(unsigned i=0;i<1000;i++){clock_us+=1000;pm_stats_usb_poll(true);pm_trace_flush();}assert(used==before);
    if(argc==2){FILE *file=fopen(argv[1],"w");assert(file);fputs(output,file);assert(!fclose(file));}
    printf("rx dump PASS: all 128 lines via real trace/7-byte partial USB writes in %ums, no loss or post-capture mutation; quiet after dump\n",report_end_ms);
}
