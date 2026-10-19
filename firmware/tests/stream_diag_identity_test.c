#include "stream_diag.h"
#include <assert.h>
#include <stdarg.h>
#include <stdio.h>
#include <string.h>
static uint64_t clock_us=100;
uint64_t time_us_64(void){return clock_us++;}
static char dump[32768];static size_t dump_used;
int pm_logf(const char *format,...){
    char line[144];va_list args;va_start(args,format);int n=vsnprintf(line,sizeof line,format,args);va_end(args);
    assert(n>0&&(size_t)n<sizeof line&&dump_used+(size_t)n<sizeof dump);
    memcpy(dump+dump_used,line,(size_t)n);dump_used+=(size_t)n;return n;
}
#include "../src/stream_diag.c"
static sample *latest(void){return &d.ring[(d.head+d.count-1)%SAMPLES];}
int main(int argc,char **argv){
    // Same frame10/frame11 reproduction as the independent review.
    pm_diag_connection(true);
    pm_diag_frame_begin(1,10,10);pm_diag_progress(25600);pm_diag_frame_done();
    pm_diag_event(PM_D_OFFER,1,10,10,0);
    pm_diag_frame_begin(1,11,11);pm_diag_progress(4096);
    pm_diag_event(PM_D_DRAW,1,10,10,0);pm_diag_draw_begin();clock_us+=39000;pm_diag_draw_done(clock_us);
    pm_diag_sample(0,0,0,23360,123);
    sample *s=latest();assert(s->frame==11&&s->session==1&&s->sequence==11&&s->assembly==4096);
    assert(s->display_frame==10&&s->display_session==1&&s->connection==1&&(s->flags&28u)==28u);
    pm_diag_event(PM_D_ORDER,9,99,0,0);pm_diag_event(PM_D_CONTEXT,9,999,0,0);
    pm_diag_sample(0,0,0,23360,124);assert(latest()->frame==11&&latest()->display_frame==10);
    puts("R1 PASS: assembling frame11/4096B and displayed frame10 remain separate; rejections cannot overwrite identity");
    uint64_t last_draw=d.last_draw;
    pm_diag_event(PM_D_RESET,0,0,0,0);pm_diag_sample(0,0,0,23360,987);s=latest();
    assert(!s->frame&&!s->session&&!s->sequence&&!s->display_frame&&!s->assembly&&!(s->flags&12u));
    assert(d.last_draw==last_draw&&d.have_draw);
    pm_diag_frame_begin(1,12,12);pm_diag_progress(2048);pm_diag_abort();pm_diag_sample(0,0,0,23360,988);
    assert(!latest()->frame&&!latest()->assembly&&!(latest()->flags&4u));
    pm_diag_event(PM_D_CLOSE,0,0,0,0);pm_diag_sample(0,0,0,0,0);assert(!(latest()->flags&28u));
    pm_diag_connection(true);pm_diag_frame_begin(2,13,13);pm_diag_progress(512);
    pm_diag_sample(0,0,0,23360,3);s=latest();assert(s->connection==2&&s->session==2&&s->frame==13&&(s->flags&20u)==20u);
    assert(d.last_draw==last_draw);
    puts("R1 PASS: abort/reset/close invalidate context; TCP generation changes while reconnect gap clock survives");
    pm_diag_usb(true);while(pm_diag_emit(11)){}
    if(argc>1){FILE *f=fopen(argv[1],"w");assert(f);assert(fwrite(dump,1,dump_used,f)==dump_used);fclose(f);}
    pm_diag_connection(false);pm_diag_connection(true);
    assert(latest()->connection==2);pm_diag_usb(false);pm_diag_sample(0,0,0,23360,1);
    assert(latest()->connection==3&&(latest()->flags&16u)&&!latest()->frame&&!latest()->display_frame);
    printf("R1 PASS: USB frozen identity immutable, next window keeps live TCP generation; RAM=%zu\n",pm_diag_ram_bytes());
}
