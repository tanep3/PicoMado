/* Production control adapter with the existing bounded fake IO, no devices. */
#define main existing_control_test_main
#include "bluetooth_control_test.c"
#undef main
_Static_assert(PM_BT_STARTUP_DIAGNOSTIC==2,"ACK wait fixture");

static void begin(void){reset();assert(command(1,1,"connect")==PM_API_PENDING);poll_bluetooth();assert(!init_calls);}
static void progress(uint32_t now){now_ms=now;startup_tcp.lastack++;startup_tcp.snd_lbb=startup_tcp.lastack;poll_bluetooth();}
static void test_rule(void){
    pm_bt_startup_gate g={0};
    pm_bt_startup_sample s={.connection=1,.ack=100,.last_byte=100,.available=true};
    assert(pm_bt_startup_observe(&g,s,0)==PM_BT_START_BASELINE);
    assert(pm_bt_startup_observe(&g,s,5000)==PM_BT_START_WAIT);
    s.ack=s.last_byte=101;
    assert(pm_bt_startup_observe(&g,s,5000)==PM_BT_START_PROGRESS);
    assert(pm_bt_startup_observe(&g,s,9999)==PM_BT_START_WAIT);
    assert(pm_bt_startup_observe(&g,s,10000)==PM_BT_START_OPEN);
    s.unacked=true;assert(pm_bt_startup_observe(&g,s,10000)==PM_BT_START_WAIT);
    s.unacked=false;s.unsent=true;assert(pm_bt_startup_observe(&g,s,10000)==PM_BT_START_WAIT);
    s.unsent=false;s.last_byte++;assert(pm_bt_startup_observe(&g,s,10000)==PM_BT_START_WAIT);
    s.connection=2;s.last_byte=s.ack;assert(pm_bt_startup_observe(&g,s,10000)==PM_BT_START_BASELINE);
    assert(pm_bt_startup_observe(&g,s,20000)==PM_BT_START_WAIT);
    s.available=false;assert(pm_bt_startup_observe(&g,s,20000)==PM_BT_START_RESET);
    s.available=true;s.ack=s.last_byte=UINT32_MAX-1;
    assert(pm_bt_startup_observe(&g,s,UINT32_MAX-100)==PM_BT_START_BASELINE);
    s.ack=s.last_byte=1;assert(pm_bt_startup_observe(&g,s,UINT32_MAX-50)==PM_BT_START_PROGRESS);
    assert(pm_bt_startup_observe(&g,s,4948)==PM_BT_START_WAIT);
    assert(pm_bt_startup_observe(&g,s,4949)==PM_BT_START_OPEN);
    s.ack=0;assert(pm_bt_startup_observe(&g,s,5000)==PM_BT_START_RESET);
    s.ack=UINT32_C(0x80000000);assert(pm_bt_startup_observe(&g,s,5100)==PM_BT_START_RESET);
    s.connection=0;assert(pm_bt_startup_observe(&g,s,5200)==PM_BT_START_RESET);
}
static void test_open_and_ready(void){
    begin();unsigned reads=startup_reads;
    for(now_ms=1;now_ms<100;now_ms++)poll_bluetooth();
    assert(startup_reads==reads&&!init_calls); /* Nonblocking, bounded sampling. */
    progress(100);now_ms=5000;poll_bluetooth();assert(!init_calls);
    ready_result=false;now_ms=5100;poll_bluetooth();assert(init_calls==1&&!connect_calls);
    for(unsigned i=0;i<20;i++){poll_bluetooth();}
    assert(init_calls==1&&!connect_calls);
    ready_result=true;poll_bluetooth();assert(stop_calls==1&&!connect_calls);
    finish_stop(1);assert(connect_calls==1&&init_calls==1);
    assert(strstr(startup_log,"event=ack_progress")&&strstr(startup_log,"event=gate_open"));
    assert(strstr(startup_log,"event=hci_on_begin")&&strstr(startup_log,"event=hci_on_return"));
    assert(strstr(startup_log,"event=hci_working")&&strstr(startup_log,"event=connect_begin"));
}
static void test_expiry(void){
    begin();now_ms=24999;poll_bluetooth();assert(!init_calls&&!reports);
    now_ms=25000;poll_bluetooth();assert(!init_calls&&reports==1&&bt_timed_out&&!bt_pending);
    progress(30000);assert(!init_calls&&reports==1);
    assert(strstr(startup_log,"event=command_expired"));
    begin();progress(19999);now_ms=24999;poll_bluetooth();assert(init_calls==1);
    now_ms=25000;poll_bluetooth();assert(bt_timed_out&&!connect_calls);
    ready_result=true;now_ms=25001;poll_bluetooth();assert(!connect_calls&&reports==1);
    begin();progress(20000);now_ms=25000;poll_bluetooth();assert(!init_calls&&bt_timed_out);
    reset();now_ms=UINT32_MAX-100;command(1,1,"connect");poll_bluetooth();
    now_ms+=25000;poll_bluetooth();assert(bt_timed_out&&!init_calls);
    begin();progress(100);init_result=false;now_ms=5100;poll_bluetooth();
    assert(init_calls==1&&reports==1&&!bt_pending&&bt_timed_out);
    poll_bluetooth();assert(init_calls==1&&reports==1&&strstr(startup_log,"event=init_failed"));
}
static void test_scope_and_loss(void){
    for(unsigned which=0;which<4;which++){
        begin();progress(100);now_ms=1000;
        if(which==0)ip_ready=false;
        if(which==1)net_ready=false;
        if(which==2)startup_tcp.valid=false;
        if(which==3)startup_tcp.connection_id=2;
        poll_bluetooth();now_ms=5100;poll_bluetooth();assert(!init_calls);
        ip_ready=net_ready=startup_tcp.valid=true;now_ms=5200;poll_bluetooth();
        progress(5300);now_ms=10300;poll_bluetooth();assert(init_calls==1);
    }
    begin();progress(100);startup_tcp.unacked=true;now_ms=5100;poll_bluetooth();assert(!init_calls);
    startup_tcp.unacked=false;startup_tcp.unsent=true;now_ms=5200;poll_bluetooth();assert(!init_calls);
    startup_tcp.unsent=false;startup_tcp.snd_lbb++;now_ms=5300;poll_bluetooth();assert(!init_calls);
    startup_tcp.lastack++;now_ms=5400;poll_bluetooth();assert(init_calls==1);
    for(unsigned which=0;which<3;which++){
        begin();progress(100);
        if(which==0)session_epoch++;
        if(which==1)incarnation++;
        if(which==2)state=0;
        now_ms=5100;poll_bluetooth();assert(!init_calls&&!bt_pending);
        state=ACTIVE;incarnation=1;session_epoch=2;poll_bluetooth();assert(!init_calls);
    }
}
static void test_supersession_and_stop(void){
    begin();progress(100);now_ms=5100;
    assert(command(2,2,"connect")==PM_API_PENDING);poll_bluetooth();assert(!init_calls);
    progress(5200);now_ms=10200;poll_bluetooth();assert(init_calls==1);
    finish_stop(2);assert(last_connect==2);
    begin();progress(100);assert(command(1,2,"connect")==PM_API_IGNORED);
    now_ms=5100;poll_bluetooth();assert(init_calls==1);
    begin();progress(100);command(2,2,"stop");poll_bluetooth();
    assert(!init_calls&&!stop_calls&&!bt_pending&&!bt_waiting&&reports==1&&strstr(last_report,"true"));
    now_ms=10000;poll_bluetooth();assert(!init_calls&&reports==1);
    assert(strstr(startup_log,"event=stop_unstarted"));
    reset();command(1,1,"stop");poll_bluetooth();assert(!init_calls&&reports==1);
}
static void test_capture_identity_and_bounds(void){
    pm_bt_startup_times t={0};
    pm_bt_startup_capture(&t,1,10,0);pm_bt_startup_capture(&t,2,20,-5);
    pm_bt_startup_capture(&t,4,30,0);pm_bt_startup_capture(&t,4,40,0);
    pm_bt_startup_capture(&t,8,50,0);
    assert(t.seen==7&&t.begin_ms==10&&t.end_ms==20&&t.result==-5&&t.working_ms==30);
    begin();progress(100);ready_result=false;now_ms=5100;poll_bluetooth();assert(init_calls==1);
    command(2,2,"connect");pm_bt_startup_capture(&runtime_times,4,5150,0);now_ms=5200;poll_bluetooth();
    assert(strstr(startup_log,"ms=5150 event=hci_working conn=1 gen=1"));
    reset();bt_received_ms=UINT32_MAX;bt_incarnation=bt_token.session=bt_token.generation=bt_token.command_id=UINT64_MAX;
    startup_tcp.connection_id=UINT64_MAX;bt_startup_command();
    bt_startup_log("command_received",UINT32_MAX,UINT64_MAX,UINT64_MAX,INT32_MIN);
    for(unsigned i=0;i<200;i++)bt_startup_log("command_expired",i,1,1,0);
    assert(bt_startup_records==96);
    char *limit=strstr(startup_log,"event=log_limit");assert(limit&&!strstr(limit+1,"event=log_limit"));
}
int main(void){
    test_rule();test_open_and_ready();test_expiry();test_scope_and_loss();
    test_supersession_and_stop();test_capture_identity_and_bounds();
    puts("BT startup: rule boundaries, actual control, ACK/IP/READY/identity, deadline, stop, markers and limits PASS");
}
