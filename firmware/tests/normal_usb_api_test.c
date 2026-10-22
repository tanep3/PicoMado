// Real main + USB parser + common serializer, with GPIO/network IO fixtures.
#define PM_CONTROL_TEST_ENTRY existing_control_cases
#define PM_TEST_REAL_USB_CONTROL 1
#define PM_CAUSAL_FIXTURE 1
#include "control_reply_test.c"
static unsigned wire_count;
static uint8_t wire_kind;
static char wire_json[768];
static void wire(uint8_t kind,const char *j){
    if(kind==7||kind==14||kind==15||kind==16){wire_count++;wire_kind=kind;snprintf(wire_json,sizeof wire_json,"%s",j);}
}
static void drain(void){for(unsigned i=0;i<1200;i++){pm_usb_control_poll(true);pm_trace_flush();}}
static void text(const char *line){for(const char *p=line;*p;p++)pm_usb_control_input(*p);pm_usb_control_input('\n');}
static void command(const char *line){text(line);drain();}
static void begin(void){
    drain();setup();memset(buttons,0,sizeof buttons);clock_ms=6000000;pressed=-1;usb_connected=true;
    trace_output=true;trace_write_cap=SIZE_MAX;trace_used=0;trace_text[0]=0;
    wire_count=0;wire_observer=wire;
#if PM_ACK_PATH_PROBE
    pm_ack_probe_init_boot(0x22);
#endif
    pm_usb_control_init(0x22,PM_BUTTON_COUNT);
    pm_usb_control_set_operations(PM_INPUT_OPERATIONS_MASK,usb_operation);
    poll_local_inputs(clock_ms,true);
}
static void expect(const char *s){assert(strstr(trace_text,s));}
static void receive_full(unsigned id,unsigned epoch){
    start(id,epoch,1,"normal");
    for(unsigned at=0;at<PM_IMAGE_BYTES;at+=256){unsigned n=PM_IMAGE_BYTES-at;if(n>256)n=256;native_chunk(id,epoch,1,at,n);}
}
static void finish_remote(void){
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
    if(drawing){panel_result=true;poll_display();}
#endif
    if(pending_render_ack)assert(send_pending_render_ack());
}
int main(void){
    begin();command("INPUT CAPS");expect("input caps b=22 ops=");
#if PM_INPUT_OPERATIONS_MASK != PM_OPERATION_MASK
    retained_pixels=true;command("INPUT DO 22 1 POWER");expect("op=POWER status=unsupported");assert(!wire_count&&!pending_standby&&retained_pixels);
    command("INPUT DO 22 2 SUB");expect("op=SUB status=dispatched");assert(wire_count==1&&wire_kind==14);
    puts("normal USB: capability restriction uses the same application API PASS");return 0;
#endif
    // Logical operations are available with no physical keys and no probes.
    const char *names[]={"SUB","UP","POWER","DOWN"};const unsigned kinds[]={14,15,7,16};
    for(unsigned i=0;i<4;i++){
        begin();char line[80];snprintf(line,sizeof line,"INPUT DO 22 1 %s",names[i]);command(line);
        assert(wire_count==1&&wire_kind==kinds[i]);expect("status=dispatched");
        command(line);assert(wire_count==1); // Retry returns the prior result.
        command(i?"INPUT DO 22 1 SUB":"INPUT DO 22 1 UP");expect("input error=conflict");assert(wire_count==1);
    }
    // GPIO resolves the gesture before the same semantic handler. Compare the
    // real GPIO path and USB path at the outbound API on the supported boards.
#if PM_BUTTON_COUNT > 0
    begin();pressed=0;poll_local_inputs(clock_ms,true);clock_ms+=35;poll_local_inputs(clock_ms,true);
#if PM_BUTTON_COUNT == 2
    for(unsigned held=0;held<settings.long_ms;held+=5){clock_ms+=5;poll_local_inputs(clock_ms,true);}
#endif
    assert(wire_count==1&&wire_kind==14);char physical[768];snprintf(physical,sizeof physical,"%s",wire_json);
    begin();command("INPUT DO 22 1 SUB");assert(wire_count==1&&!strcmp(wire_json,physical));
#endif
    begin();command("INPUT DO 22 2 UP");command("INPUT DO 22 1 DOWN");expect("input error=stale_id");assert(wire_count==1);
    command("INPUT DO 22 0 SUB");command("INPUT DO 23 3 SUB");command("INPUT DO 22 -1 SUB");
    command("INPUT DO 22 10000000000000000 SUB");command("INPUT DO 22 3 UNKNOWN");
    command("INPUT PRESS 22 3 SUB");command("INPUT RELEASE 22 3 SUB");assert(wire_count==1);
    assert(perform_operation((pm_operation){0},2,false)==PM_INPUT_INVALID);
    assert(perform_operation((pm_operation){255},2,false)==PM_INPUT_INVALID&&wire_count==1);
    command("INPUT DO 22 3 DOWN");assert(wire_count==2&&wire_kind==16); // A real second click.
    // Reconnecting clients learn the next ID without issuing trial operations.
    begin();command("INPUT DO 22 100 UP");command("INPUT CAPS");expect("last_request_id=100\n");
    command("INPUT DO 22 101 DOWN");assert(wire_count==2);
    command("INPUT DO 22 ffffffffffffffff SUB");command("INPUT CAPS");expect("last_request_id=ffffffffffffffff\n");
    command("INPUT DO 22 0 UP");command("INPUT DO 22 1 UP");assert(wire_count==3);
    // Rejections happen before retained image ownership or state changes.
    begin();retained_pixels=true;uint64_t previous_owner=owner_epoch;
    command("INPUT DO 22 0 SUB");command("INPUT DO 23 1 SUB");command("INPUT DO 22 1 NOPE");
    assert(perform_operation((pm_operation){0},2,false)==PM_INPUT_INVALID);
    assert(retained_pixels&&owner_epoch==previous_owner&&!wire_count&&!pending_standby);
    begin();state=WIFI;command("INPUT DO 22 1 UP");expect("status=ignored");assert(!wire_count);
    begin();net_send_rejected=true;command("INPUT DO 22 1 SUB");expect("status=failed");assert(!wire_count&&!switching.waiting);
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
    begin();panel_standby=true;command("INPUT DO 22 1 SUB");expect("status=busy");assert(!wire_count);
    panel_standby=false;command("INPUT DO 22 1 SUB");assert(!wire_count);
    command("INPUT DO 22 2 SUB");assert(wire_count==1);
#endif
    begin();state=STANDBY;unsigned old_joins=joins;
    command("INPUT DO 22 1 POWER");assert(state==WIFI&&joins==old_joins+1&&!wire_count);expect("status=dispatched");
#if PM_ACK_PATH_PROBE
    begin();command("INPUT DO 22 1 SUB");assert(wire_count==1);
    command("AP2 ARM 22 1");clock_ms+=30;drain();assert(pm_ack_probe_read_summary().state==PM_AP_PREPARED);
    command("AP2 FREEZE 22 1");clock_ms+=30;drain();assert(pm_ack_probe_read_summary().state==PM_AP_FROZEN);
    char release[96];snprintf(release,sizeof release,"AP2 RELEASE 22 1 %x",pm_ack_probe_read_summary().digest);
    command(release);clock_ms+=30;drain();assert(pm_ack_probe_read_summary().state==PM_AP_IDLE);
    command("INPUT DO 22 1 SUB");assert(wire_count==1);
#endif
#if PM_BUTTON_COUNT > 0
    // Main serializes the GPIO event first, then the ordinary USB request.
    // The request and a USB disconnect never own/reset GPIO hold state.
    begin();unsigned power_key=PM_BUTTON_COUNT==4?2:1;
    pressed=(int)power_key;poll_local_inputs(clock_ms,true);clock_ms+=35;poll_local_inputs(clock_ms,true);
#if PM_BUTTON_COUNT == 2
    for(unsigned held=0;held<settings.long_ms;held+=5){clock_ms+=5;poll_local_inputs(clock_ms,true);}
#endif
    assert(wire_count==1&&wire_kind==7&&buttons[power_key].stable);
    command("INPUT DO 22 1 UP");assert(wire_count==2&&wire_kind==15&&buttons[power_key].stable);
    pm_usb_control_poll(false);usb_connected=false;clock_ms+=5;poll_local_inputs(clock_ms,true);
    assert(buttons[power_key].stable&&wire_count==2);
#endif
    // Both just-before and exact-deadline CRC arrival suppress unstarted UI.
    for(unsigned delay=198;delay<=200;delay+=2){
        begin();command("INPUT DO 22 1 SUB");clock_ms+=delay;receive_full(200+delay,2);
        poll_switch_feedback();assert(!feedback_shows);finish_remote();assert(!switching.waiting);
    }
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
    begin();receive_full(500,1);assert(drawing);command("INPUT DO 22 1 SUB");
    clock_ms+=200;poll_switch_feedback();assert(feedback_shows==1);
    receive_full(501,2);assert(pm_switch_received(&switching)&&!switching.hint_requested);
    command("INPUT DO 22 2 SUB");assert(!pm_switch_received(&switching));
    panel_result=true;poll_display();clock_ms+=200;poll_switch_feedback();assert(feedback_shows==2);
    receive_full(502,3);finish_remote();assert(!switching.waiting);
#endif
    // A completed semantic request enters the already accepted 200 ms model.
    begin();command("INPUT DO 22 1 SUB");clock_ms+=199;poll_switch_feedback();assert(!feedback_shows);
    clock_ms++;poll_switch_feedback();assert(feedback_shows==1);
    command("INPUT DO 22 1 SUB");assert(wire_count==1&&feedback_shows==1);
    reply(10,1,1,1,"normal",false);assert(!switching.waiting&&feedback_restores==1);
    // A partial line is canceled by disconnect, and completed requests are not
    // replayed on reconnect. Changed boot rejects old client requests.
    begin();for(const char *p="INPUT DO 22";*p;p++)pm_usb_control_input(*p);
    pm_usb_control_poll(false);command(" 1 UP");assert(!wire_count);
    command("INPUT DO 22 1 UP");assert(wire_count==1);
    pm_usb_control_poll(false);command("INPUT DO 22 1 UP");assert(wire_count==1);
    command("INPUT CAPS");expect("last_request_id=1\n");
    pm_usb_control_init(0x33,PM_BUTTON_COUNT);pm_usb_control_set_operations(PM_INPUT_OPERATIONS_MASK,usb_operation);
    poll_local_inputs(clock_ms,true);command("INPUT DO 22 2 UP");expect("input error=boot");assert(wire_count==1);
    // No USB space: bounded queuing, no automatic repeats, then exact complete
    // lines through the same partial writer even alongside ordinary logs.
    begin();trace_write_cap=0;text("INPUT DO 22 1 UP");text("INPUT DO 22 2 DOWN");text("INPUT DO 22 3 SUB");
    assert(wire_count==2);drain();assert(!trace_used&&wire_count==2);
    trace_write_cap=3;pm_logf("ordinary shared line\n");drain();expect("id=1 op=UP status=dispatched\n");expect("id=2 op=DOWN status=dispatched\n");
    command("INPUT DO 22 2 DOWN");assert(wire_count==2);command("INPUT DO 22 3 SUB");assert(wire_count==3);
    expect("id=3 op=SUB status=dispatched\n");
    begin();pm_usb_control_input(1);command("INPUT DO 22 1 SUB");assert(!wire_count);command("INPUT DO 22 1 UP");assert(wire_count==1);
    begin();char too_long[130];memset(too_long,'A',sizeof too_long-1);too_long[sizeof too_long-1]=0;
    command(too_long);expect("usb error=length");assert(!wire_count);command("INPUT DO 22 1 UP");assert(wire_count==1);
    printf("normal USB API %s keys=%u trace=%u: semantic shared dispatch, IDs/capabilities/state, no probes, partial/zero output, reconnect, 200ms PASS\n",PM_DISPLAY_CAPABILITY,PM_BUTTON_COUNT,PM_TRACE_ENABLE);
}
