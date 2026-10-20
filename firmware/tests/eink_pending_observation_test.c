// Observe the actual E-ink API/pending/physical-result path through the real EV1 encoder.
#define PM_CAUSAL_FIXTURE 1
#define PM_CONTROL_TEST_ENTRY established_control_cases
#include "control_reply_test.c"
static FILE *events;
static void drain(const char *stage){
    for(unsigned i=0;i<1024;i++)pm_trace_flush();
    fprintf(events,"STAGE %s\n",stage);
    if(trace_used)assert(fwrite(trace_text,1,trace_used,events)==trace_used);
    trace_used=0;trace_text[0]=0;
}
int main(int argc,char **argv){
    assert(argc==2&&PM_DISPLAY_TYPE==PM_DISPLAY_EINK);
    events=fopen(argv[1],"wb");assert(events);
    setup();trace_output=true;usb_connected=true;
    pm_usb_control_init(0x1234,PM_BUTTON_COUNT);pm_usb_control_poll(true);pm_observation_enable(true);
    start(101,1,1,"normal");drain("first_header");
    chunk(101,1,1,0,PM_IMAGE_BYTES);drain("first_ready");
    assert(drawing&&drawing_token.frame_id==101&&!pending_render_ack);
    start(202,2,1,"normal");drain("latest_header");
    chunk(202,2,1,0,PM_IMAGE_BYTES);drain("latest_ready");
    assert(drawing&&drawing_token.frame_id==101&&pending_frame.phase==PM_EINK_READY);
    char token[400];token_json(token,sizeof token,303,3,1);
    snprintf(json,sizeof json,"{\"type\":\"frame_start\",\"data\":{%s,\"width\":251,\"height\":122,\"format\":\"mono1\",\"byte_len\":3813,\"crc32\":0}}",token);
    process(12,json);drain("invalid_header");
    assert(pending_frame.phase==PM_EINK_READY&&pending_frame.frame.token.frame_id==202);
    panel_result=true;poll_display();drain("old_done");
    assert(drawing&&drawing_token.frame_id==202&&!pending_render_ack&&!frame_renders);
    panel_result=true;poll_display();assert(pending_render_token.frame_id==202);
    assert(send_pending_render_ack()&&frame_renders==1);drain("new_done");
    start(900,3,1,"normal");drain("replay_original_header");
    chunk(900,3,1,0,PM_IMAGE_BYTES);drain("replay_original_ready");
    assert(drawing&&drawing_token.frame_id==900);
    start(900,3,1,"normal");drain("replay_header");
    chunk(900,3,1,0,PM_IMAGE_BYTES);drain("replay_ready");
    assert(pending_frame.phase==PM_EINK_READY&&drawing);
    panel_result=true;poll_display();drain("replay_original_done");
    assert(!drawing&&pending_render_ack&&pending_render_token.frame_id==900&&pending_frame.phase==PM_EINK_EMPTY);
    assert(send_pending_render_ack()&&frame_renders==2);drain("replay_ack");
    poll_display();drain("replay_idle");assert(!drawing&&!pending_render_ack);
    assert(!pm_observation_read().dropped);assert(fclose(events)==0);
    puts("PASS EV1 actual E-ink API_PENDING/rejection and old/new physical frame identity");
}
