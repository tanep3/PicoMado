/* Diagnostic adapter; all formatting stays outside the shared runtime lock. */
#if PM_BT_STARTUP_DIAGNOSTIC
static pm_bt_startup_gate bt_startup_gate;
static uint32_t bt_startup_sampled_ms;
static bool bt_startup_sampled,bt_startup_tracking,bt_startup_attempted;
static uint64_t bt_startup_command_connection,bt_startup_first_connection,bt_startup_first_generation;
static unsigned bt_startup_records;
static uint8_t bt_startup_seen;
static bool bt_startup_record(uint32_t ms){
    if(bt_startup_records>=96)return false;
    if(++bt_startup_records==96){
        pm_logf("bt boot ms=%lu event=log_limit further=unreported\n",(unsigned long)ms);
        return false;
    }
    return true;
}
static void bt_startup_log(const char *event,uint32_t ms,uint64_t connection,uint64_t gen,int32_t value){
    if(bt_startup_record(ms))pm_logf("bt boot ms=%lu event=%s conn=%llu gen=%llu value=%ld\n",
        (unsigned long)ms,event,(unsigned long long)connection,(unsigned long long)gen,(long)value);
}
static void bt_startup_cancel(const char *reason){
    if(bt_startup_tracking)bt_startup_log(reason,to_ms_since_boot(get_absolute_time()),
        bt_startup_command_connection,bt_token.generation,0);
    bt_startup_gate=(pm_bt_startup_gate){0};bt_startup_tracking=false;bt_startup_sampled=false;
}
static void bt_startup_command(void){
    pm_net_tcp_status s;pm_net_tcp_snapshot(&s);
    bt_startup_command_connection=s.connection_id;
    bt_startup_gate=(pm_bt_startup_gate){0};bt_startup_sampled=false;bt_startup_tracking=true;
    if(bt_startup_record(bt_received_ms))pm_logf(
        "bt boot token ms=%lu inc=%llu session=%llu gen=%llu command=%llu\n",
        (unsigned long)bt_received_ms,(unsigned long long)bt_incarnation,
        (unsigned long long)bt_token.session,(unsigned long long)bt_token.generation,
        (unsigned long long)bt_token.command_id);
    bt_startup_log("command_received",bt_received_ms,s.connection_id,bt_token.generation,(int32_t)bt_action);
}
static void bt_startup_first_attempt(void){
    if(bt_startup_attempted)return;
    bt_startup_attempted=true;bt_startup_first_connection=bt_startup_command_connection;
    bt_startup_first_generation=bt_token.generation;
}
static void bt_startup_markers(void){
    if(!bt_startup_attempted||bt_startup_seen==7||bt_startup_records>=96)return;
    pm_bt_startup_times t;
    cyw43_arch_lwip_begin();pm_bt_runtime_startup_snapshot(&t);cyw43_arch_lwip_end();
    uint8_t fresh=t.seen&~bt_startup_seen;bt_startup_seen|=t.seen;
    if(fresh&1)bt_startup_log("hci_on_begin",t.begin_ms,bt_startup_first_connection,bt_startup_first_generation,0);
    if(fresh&2)bt_startup_log("hci_on_return",t.end_ms,bt_startup_first_connection,bt_startup_first_generation,t.result);
    if(fresh&4)bt_startup_log("hci_working",t.working_ms,bt_startup_first_connection,bt_startup_first_generation,0);
    if((t.seen&2)&&t.result)bt_startup_seen=7; /* Failed power-on cannot reach WORKING. */
}
static bool bt_startup_admit(void){
#if PM_BT_STARTUP_DIAGNOSTIC == 2
    uint32_t now=to_ms_since_boot(get_absolute_time());
    if(bt_startup_sampled&&(uint32_t)(now-bt_startup_sampled_ms)<100)return false;
    bt_startup_sampled=true;bt_startup_sampled_ms=now;
    pm_net_tcp_status s;pm_net_tcp_snapshot(&s);
    bool available=s.valid&&pm_net_state()==PM_NET_READY&&
        cyw43_tcpip_link_status(&cyw43_state,CYW43_ITF_STA)==CYW43_LINK_UP;
    /* A shared-lock/query wait may cross the non-extending command deadline. */
    now=to_ms_since_boot(get_absolute_time());bt_startup_sampled_ms=now;
    if((uint32_t)(now-bt_received_ms)>=25000)return false;
    pm_bt_startup_step step=pm_bt_startup_observe(&bt_startup_gate,(pm_bt_startup_sample){
        .connection=s.connection_id,.ack=s.lastack,.last_byte=s.snd_lbb,
        .available=available,.unsent=s.unsent,.unacked=s.unacked},now);
    const char *event=step==PM_BT_START_BASELINE?"gate_wait":step==PM_BT_START_RESET?"gate_reset":
        step==PM_BT_START_PROGRESS?"ack_progress":step==PM_BT_START_OPEN?"gate_open":NULL;
    if(event){
        bt_startup_log(event,now,s.connection_id,bt_token.generation,(int32_t)(now-bt_received_ms));
        if(bt_startup_record(now))pm_logf("bt boot tcp ms=%lu conn=%llu ack=%lu end=%lu queued=%u ready=%u\n",
            (unsigned long)now,(unsigned long long)s.connection_id,(unsigned long)s.lastack,
            (unsigned long)s.snd_lbb,(s.unsent?1u:0u)|(s.unacked?2u:0u),available?1u:0u);
    }
    if(step!=PM_BT_START_OPEN)return false;
    bt_startup_command_connection=s.connection_id;
#endif
    return true;
}
#endif
