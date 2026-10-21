// Permanent form of independent P2-01: real main/parser and existing IO fixture.
#define PM_CONTROL_TEST_ENTRY existing_control_cases
#include "control_reply_test.c"
#if PM_DISPLAY_TYPE != PM_DISPLAY_EINK
#error "This regression exercises deferred E-ink refresh ownership"
#endif
static void complete_image(unsigned id,unsigned epoch){
    start(id,epoch,1,"normal");
    for(unsigned at=0;at<PM_IMAGE_BYTES;at+=256){
        unsigned n=PM_IMAGE_BYTES-at;if(n>256)n=256;
        native_chunk(id,epoch,1,at,n);
    }
}
int main(int argc,char **argv){
    unsigned route=argc>1?(unsigned)atoi(argv[1]):0;assert(route<=2);
    setup();clock_ms=6000000;
    complete_image(100,1);assert(drawing); // Old physical refresh cannot abort.
    button(0,false);clock_ms+=200;poll_switch_feedback();
    assert(feedback_shows==1&&switching.hint_requested);
    unsigned replaced=feedback_replaces;
    complete_image(101,2);assert(drawing&&pending_frame.phase==PM_EINK_READY);
    assert(pm_switch_received(&switching)&&feedback_replaces>replaced);
    if(route==0)start(102,2,1,"normal"); // Candidate bytes are replaced.
    else button(route==1?3:0,false); // Navigation or a second SUB supersedes it.
    assert(switching.waiting&&!pm_switch_received(&switching));
    panel_result=true;poll_display();assert(!drawing);
    if(route){clock_ms+=199;poll_switch_feedback();assert(feedback_shows==1);clock_ms++;}
    poll_switch_feedback();
    printf("route=%u waiting=%u received=%u hint_requested=%u hint_show_calls=%u replacement_calls=%u drawing=%u\n",
        route,switching.waiting,pm_switch_received(&switching),switching.hint_requested,
        feedback_shows,feedback_replaces,drawing);
    if(feedback_shows!=2){puts("FAIL: canceled unstarted hint was not re-requested");return 2;}
    clock_ms+=1000;poll_switch_feedback();assert(feedback_shows==2);
    // The re-request is canceled once a real image is available, and the
    // existing physical result completes it without a local remote ACK.
    complete_image(103,route?3:2);assert(drawing&&pm_switch_received(&switching));
    panel_result=true;poll_display();assert(!switching.waiting&&pending_render_ack);
    assert(send_pending_render_ack());assert(frame_renders==1);
    poll_switch_feedback();assert(feedback_shows==2);
    puts("PASS canceled hint: old refresh, candidate eviction/navigation/SUB, original/new deadline, single re-request, real completion");
    return 0;
}
