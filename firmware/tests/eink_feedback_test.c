// Reuse the production-driver SPI/BUSY fixture and its existing regression
// suite; feedback checks below use only the public display API.
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wreturn-type"
#define main pm_existing_eink_test_main
#include "eink_test.c"
#undef main
#pragma GCC diagnostic pop
#include "switch_bitmap.h"

static void no_remote_result(void){bool ok;assert(!pm_display_take_result(&ok));}
static void remote_done(void){
    bool ok;poll_for(1000);assert(!pm_display_busy());
    assert(pm_display_take_result(&ok)&&ok);no_remote_result();
}
static void reach_refresh(unsigned previous){
    unsigned limit=clock_ms+1000;
    while(refreshes==previous && clock_ms<limit)poll_for(5);
    assert(refreshes==previous+1);
}
static void reach_ram_write(void){
    // Stop during WRITE_RAM, before any physical refresh command.
    unsigned previous=refreshes,limit=clock_ms+1000;
    size_t previous_writes=writes;
    while(clock_ms<limit){
        poll_for(5);
        if(writes>previous_writes && command==0x24 && written>0 && written<4000)break;
    }
    assert(clock_ms<limit && refreshes==previous);
}
int main(void){
    (void)pm_existing_eink_test_main();
    if(!PM_EINK_PANEL_CONNECTED)return 0;
    static uint8_t pixels[PM_IMAGE_BYTES],expected[4000],hint[PM_IMAGE_BYTES],hint_ram[4000];
    memset(pixels,0xa5,sizeof pixels);pm_eink_pack_landscape(pixels,expected);
    memset(hint,0xff,sizeof hint);
    for(unsigned y=0;y<PM_H;y++)for(unsigned x=0;x<PM_W;x++)
        if(pm_switch_bitmap_ink(x,y,PM_W,PM_H)){
            unsigned b=y*PM_W+x;hint[b/8]&=(uint8_t)~(0x80u>>(b&7));
        }
    pm_eink_pack_landscape(hint,hint_ram);
    assert(pm_display_frame(pixels,sizeof pixels));remote_done();
    unsigned before=refreshes;

    // A complete frame before the deadline cancels a still-unstarted intent.
    pm_display_feedback(PM_DISPLAY_FEEDBACK_SHOW);
    pm_display_feedback(PM_DISPLAY_FEEDBACK_REPLACE);poll_for(1000);
    assert(refreshes==before && !pm_display_busy());no_remote_result();
    pm_display_feedback(PM_DISPLAY_FEEDBACK_SHOW);poll_for(40);
    pm_display_feedback(PM_DISPLAY_FEEDBACK_RESTORE);poll_for(1000);
    assert(refreshes==before && !pm_display_busy());no_remote_result();

    // SHOW cannot overtake a queued remote frame, or become an extra repaint
    // while an old remote frame is physically active.
    assert(pm_display_frame(pixels,sizeof pixels));
    pm_display_feedback(PM_DISPLAY_FEEDBACK_SHOW);poll_for(5);
    pm_display_feedback(PM_DISPLAY_FEEDBACK_REPLACE);remote_done();
    assert(refreshes==++before && !memcmp(output,expected,sizeof output));
    assert(pm_display_frame(pixels,sizeof pixels));poll_for(5);
    pm_display_feedback(PM_DISPLAY_FEEDBACK_SHOW);poll_for(100);
    assert(refreshes==before);
    pm_display_feedback(PM_DISPLAY_FEEDBACK_REPLACE);remote_done();assert(refreshes==++before);

    // Cancel during reset, then during RAM transfer. Neither preparation is a
    // visible update; the real frame starts its own full reset and refresh.
    pm_display_feedback(PM_DISPLAY_FEEDBACK_SHOW);poll_for(40);
    assert(refreshes==before);assert(pm_display_frame(pixels,sizeof pixels));remote_done();
    assert(refreshes==++before);
    pm_display_feedback(PM_DISPLAY_FEEDBACK_SHOW);reach_ram_write();
    pm_display_feedback(PM_DISPLAY_FEEDBACK_REPLACE);
    assert(!pm_display_busy());assert(pm_display_frame(pixels,sizeof pixels));remote_done();
    assert(refreshes==++before && !memcmp(output,expected,sizeof output));

    // Local feedback has no remote result and retains the exact source image.
    pm_display_feedback(PM_DISPLAY_FEEDBACK_SHOW);poll_for(1000);
    assert(refreshes==++before && !memcmp(output,hint_ram,sizeof output));no_remote_result();
    pm_display_feedback(PM_DISPLAY_FEEDBACK_SHOW);poll_for(1000);
    assert(refreshes==before);no_remote_result();
    // A newer wait can supersede an old no-op restoration before it starts.
    pm_display_feedback(PM_DISPLAY_FEEDBACK_RESTORE);reach_ram_write();
    pm_display_feedback(PM_DISPLAY_FEEDBACK_SHOW);poll_for(1000);
    assert(refreshes==before&&!pm_display_busy());no_remote_result();
    pm_display_feedback(PM_DISPLAY_FEEDBACK_RESTORE);poll_for(1000);
    assert(refreshes==++before && !memcmp(output,expected,sizeof output));no_remote_result();
    pm_display_feedback(PM_DISPLAY_FEEDBACK_RESTORE);poll_for(1000);
    assert(refreshes==before);no_remote_result();

    // Once 0x20 was sent, BUSY owns the physical refresh. A real image queues
    // behind it, cancels future hints and wins over a no-op restoration.
#if PM_TRACE_ENABLE
    capture_spi=true;spi_frame_count=0;
#endif
    pm_display_feedback(PM_DISPLAY_FEEDBACK_SHOW);reach_refresh(before);before++;
    busy=true;
    memset(pixels,0x3c,sizeof pixels);pm_eink_pack_landscape(pixels,expected);
#if PM_TRACE_ENABLE
    test_frame_context=9902;
#endif
    assert(pm_display_frame(pixels,sizeof pixels));
    pm_display_feedback(PM_DISPLAY_FEEDBACK_RESTORE);poll_for(100);
    assert(refreshes==before && pm_display_busy());no_remote_result();
    busy=false;remote_done();assert(refreshes==++before);
    assert(!memcmp(output,expected,sizeof output));
#if PM_TRACE_ENABLE
    assert(spi_frame_count==4&&spi_frames[0]==0&&spi_frames[1]==0&&spi_frames[2]==9902&&spi_frames[3]==9902);
    capture_spi=false;
#endif

    // A no-op after physical start restores only after that refresh drains.
    pm_display_feedback(PM_DISPLAY_FEEDBACK_SHOW);reach_refresh(before);before++;
    busy=true;pm_display_feedback(PM_DISPLAY_FEEDBACK_RESTORE);poll_for(100);
    assert(refreshes==before);no_remote_result();
    busy=false;poll_for(1500);assert(refreshes==++before && !pm_display_busy());
    assert(!memcmp(output,expected,sizeof output));no_remote_result();

    // Restoration preparation is also replaceable by newly arrived pixels.
    pm_display_feedback(PM_DISPLAY_FEEDBACK_SHOW);poll_for(1000);assert(refreshes==++before);
    pm_display_feedback(PM_DISPLAY_FEEDBACK_RESTORE);reach_ram_write();
    assert(pm_display_frame(pixels,sizeof pixels));remote_done();assert(refreshes==++before);
    assert(!memcmp(output,expected,sizeof output));

    // Recovery status and standby own the display; hints cannot resume later.
    pm_display_feedback(PM_DISPLAY_FEEDBACK_SHOW);reach_ram_write();
    pm_display_status("SERVER","CONNECTING");poll_for(1000);
    assert(refreshes==++before);no_remote_result();
    pm_display_feedback(PM_DISPLAY_FEEDBACK_SHOW);poll_for(40);
    pm_display_standby();poll_for(1000);assert(refreshes==before&&!pm_display_busy());
    pm_display_wake();poll_for(1000);assert(refreshes==before);no_remote_result();
    pm_display_feedback(PM_DISPLAY_FEEDBACK_SHOW);busy=true;
    poll_for(PM_EINK_BUSY_TIMEOUT_MS+100);busy=false;
    assert(refreshes==before&&!pm_display_busy());no_remote_result();

    // A local full refresh invalidates the clock base; a subsequent clock frame
    // must establish old/new RAM rather than using a stale partial baseline.
    unsigned full_before=full_refreshes;
    assert(pm_display_clock_frame(pixels,sizeof pixels,10,200,true));remote_done();
    pm_display_feedback(PM_DISPLAY_FEEDBACK_SHOW);poll_for(1000);no_remote_result();
    assert(pm_display_clock_frame(pixels,sizeof pixels,10,201,false));remote_done();
    assert(full_refreshes==full_before+3);
    puts("eink feedback: retained source, local-only results, remote priority, cancellable preparation, BUSY drain, no-op restore and clock/recovery isolation PASS");
    return 0;
}
