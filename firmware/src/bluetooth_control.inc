/* Main-loop IO adapter: never send socket data from the BT callback. */
#if PM_BLUETOOTH_AUDIO
static bool send_media_input(pm_media_operation operation){
    const char *name=pm_media_operation_name(operation);
    if(!name || !pm_media_input_allowed(&media_input_session,incarnation,session_epoch,state==ACTIVE))return false;
    int n=snprintf(outbound_json,sizeof outbound_json,
        "{\"type\":\"media_input\",\"data\":{\"incarnation\":%llu,\"session_epoch\":%llu,\"operation\":\"%s\"}}",
        (unsigned long long)incarnation,(unsigned long long)session_epoch,name);
    return n>0&&(size_t)n<sizeof outbound_json&&pm_net_send(27,outbound_json);
}
typedef struct {uint64_t session,generation,command_id,expires_ms;} pm_bt_command_token;
static pm_bt_command_token bt_token;
static uint64_t bt_incarnation;
static unsigned bt_action;
static uint8_t bt_target[6];
static bool bt_initialized,bt_pending,bt_preparing,bt_waiting,bt_timed_out;
static uint32_t bt_received_ms;
static pm_bluetooth_input bt_input;
static uint32_t bt_input_epoch;
static bool bt_report(const char *report){
    int n=snprintf(outbound_json,sizeof outbound_json,
        "{\"type\":\"bluetooth_report\",\"data\":{\"incarnation\":%llu,\"session_epoch\":%llu,\"token\":{\"session\":%llu,\"generation\":%llu,\"command_id\":%llu,\"expires_ms\":%llu},\"report\":%s}}",
        (unsigned long long)bt_incarnation,(unsigned long long)bt_token.session,
        (unsigned long long)bt_token.session,(unsigned long long)bt_token.generation,
        (unsigned long long)bt_token.command_id,(unsigned long long)bt_token.expires_ms,report);
    return n>0&&(size_t)n<sizeof outbound_json&&pm_net_send(21,outbound_json);
}
static pm_api_result api_bluetooth(uint8_t kind,const char *j){
    (void)kind;uint64_t inc,session;pm_bt_command_token t;
    if(state!=ACTIVE||!number(j,"incarnation",&inc)||inc!=incarnation||
       !number(j,"session_epoch",&session)||session!=session_epoch||
       !number(j,"session",&t.session)||t.session!=session||
       !number(j,"generation",&t.generation)||!t.generation||
       !number(j,"command_id",&t.command_id)||!t.command_id||
       !number(j,"expires_ms",&t.expires_ms))return PM_API_REJECTED;
    if(bt_incarnation==inc&&bt_token.session==session&&(t.generation<=bt_token.generation||t.command_id<=bt_token.command_id))return PM_API_IGNORED;
    unsigned action=string_equals(j,"action","stop")?1:string_equals(j,"action","discover")?2:string_equals(j,"action","connect")?3:0;
    if(!action)return PM_API_REJECTED;
    uint8_t address[6]={0};
    if(action==3){
        const char *p=strstr(j,"\"target\"");if(!p||(p=strchr(p,':'))==NULL)return PM_API_REJECTED;
        ++p;while(*p==' ')++p;if(*p++!='"')return PM_API_REJECTED;
        for(unsigned i=0;i<6;i++){
            unsigned v=0;for(unsigned n=0;n<2;n++){char c=*p++;unsigned x=c>='0'&&c<='9'?c-'0':c>='A'&&c<='F'?c-'A'+10:c>='a'&&c<='f'?c-'a'+10:16;if(x==16)return PM_API_REJECTED;v=v*16+x;}address[i]=(uint8_t)v;
            if(*p++!=(i==5?'"':':'))return PM_API_REJECTED;
        }
    }
    bt_token=t;bt_incarnation=inc;bt_action=action;memcpy(bt_target,address,6);bt_pending=true;bt_preparing=false;bt_waiting=true;bt_timed_out=false;
    bt_received_ms=to_ms_since_boot(get_absolute_time());
    pm_bluetooth_input_reset(&bt_input,t.generation,false);
    return PM_API_PENDING;
}
static void poll_bluetooth(void){
    if(!bt_pending&&!bt_initialized)return;
    if(state!=ACTIVE||bt_token.session!=session_epoch||bt_incarnation!=incarnation){
        cyw43_arch_lwip_begin();if(bt_initialized)pm_bt_runtime_stop(0);cyw43_arch_lwip_end();
        bt_pending=false;pm_bluetooth_input_reset(&bt_input,0,false);return;
    }
    pm_bluetooth_input_expire(&bt_input,to_ms_since_boot(get_absolute_time()));
    /* expires_ms belongs to the host clock and is echoed only. A local budget
     * bounds deferred readiness/stop/connection work independently. */
    if(bt_waiting&&bt_initialized){
        static uint32_t last_link_snapshot_ms;
        uint32_t now=to_ms_since_boot(get_absolute_time());
        if((uint32_t)(now-last_link_snapshot_ms)>=2000){
            last_link_snapshot_ms=now;pm_bt_link_snapshot d;
            cyw43_arch_lwip_begin();pm_bt_runtime_link_snapshot(&d);cyw43_arch_lwip_end();
            pm_logf("bt setup cid=%u exists=%u conn=%d sig=%u source=%d initiator=%d outgoing=%u discover=%u configured=%u have=%u endpoint=%d media=%u\n",d.cid,d.exists,d.connection,d.signaling_cid,d.source,d.initiator,d.outgoing,d.discover,d.configured,d.have_config,d.endpoint,d.media_cid);
            /* Keep each complete record below trace.c TEXT_CAP (144 bytes),
             * including maximum generation/CID values. No queue size increase. */
            pm_logf("bt config generation=%llu cid=%u configuration=%d endpoint_valid=%u seid=%u active_match=%u\n",(unsigned long long)d.generation,d.cid,d.configuration,d.endpoint_valid,d.local_seid,d.active_matches);
            pm_logf("bt endpoint generation=%llu cid=%u initiator=%d acceptor=%d\n",(unsigned long long)d.generation,d.cid,d.endpoint_initiator,d.endpoint_acceptor);
            pm_logf("bt transaction generation=%llu cid=%u labels=%u/%u signals=%u/%u packet_labels=%u/%u\n",(unsigned long long)d.generation,d.cid,d.initiator_label,d.acceptor_label,d.initiator_signal,d.acceptor_signal,d.initiator_packet_label,d.acceptor_packet_label);
        }
    }
    if(bt_waiting&&(uint32_t)(to_ms_since_boot(get_absolute_time())-bt_received_ms)>=25000){
        bt_waiting=false;bt_pending=false;bt_timed_out=true;
        cyw43_arch_lwip_begin();if(bt_initialized)pm_bt_runtime_stop(bt_token.generation);cyw43_arch_lwip_end();
        pm_bluetooth_input_reset(&bt_input,bt_token.generation,false);
        (void)bt_report("{\"event\":\"complete\",\"success\":false}");return;
    }
    cyw43_arch_lwip_begin();
    bool init_failed=false;
    if(!bt_initialized){bt_initialized=pm_bt_runtime_init();init_failed=!bt_initialized;}
    bool ready=bt_initialized&&pm_bt_runtime_ready();
    uint8_t status=0;
    if(bt_pending&&ready){
        if(bt_action!=1&&!bt_preparing){
            /* Superseding an operation must release its radio/link before changing
             * runtime generation. Stop is asynchronous; ready() stays false until
             * inquiry and both connection owners have quiesced. */
            bt_preparing=true;pm_bt_runtime_stop(bt_token.generation);
        }else{
            bt_pending=false;
            if(bt_action==1)pm_bt_runtime_stop(bt_token.generation);
            else if(bt_action==2)status=pm_bt_runtime_scan(bt_token.generation);
            else status=pm_bt_runtime_connect_at(bt_token.generation,bt_target,bt_received_ms);
        }
    }
    pm_bt_event e;bool have=bt_initialized&&pm_bt_runtime_next(&e);
    cyw43_arch_lwip_end();
    if(init_failed){
        bt_waiting=false;bt_pending=false;bt_timed_out=true;
        (void)bt_report("{\"event\":\"complete\",\"success\":false}");return;
    }
    if(status){bt_waiting=false;(void)bt_report("{\"event\":\"complete\",\"success\":false}");return;}
    if(!have||e.generation!=bt_token.generation||bt_timed_out)return;
    if(e.kind==PM_BT_LINK_DIAGNOSTIC){
        if(e.detail3==4)pm_logf("bt discover_wait generation=%llu cid=%ld waiting=%ld evidence=state_only\n",(unsigned long long)e.generation,(long)e.detail1,(long)e.detail2);
        else pm_logf("bt link generation=%llu status=%u old_cid=%ld new_cid=%ld phase=%ld\n",(unsigned long long)e.generation,e.status,(long)e.detail1,(long)e.detail2,(long)e.detail3);
    }else if(e.kind==PM_BT_FOUND){
        char name[128],report[300];size_t n=0;
        for(size_t i=0;i<sizeof e.name&&e.name[i]&&n+2<sizeof name;i++){
            unsigned char c=e.name[i];if(c<32||c>=127)continue;
            if(c=='"'||c=='\\')name[n++]='\\';
            name[n++]=(char)c;
        }name[n]=0;
        snprintf(report,sizeof report,"{\"event\":\"candidate\",\"candidate\":{\"address\":\"%02X:%02X:%02X:%02X:%02X:%02X\",\"name\":\"%s\",\"a2dp\":null}}",e.address[0],e.address[1],e.address[2],e.address[3],e.address[4],e.address[5],name);
        (void)bt_report(report);
    }else if(e.kind==PM_BT_CONNECTED){
        bt_waiting=false;
        pm_bluetooth_input_reset(&bt_input,e.generation,true);
        cyw43_arch_lwip_begin();pm_bt_runtime_enable_input(true);cyw43_arch_lwip_end();
        (void)bt_report("{\"event\":\"complete\",\"success\":true}");
    }else if(e.kind==PM_BT_SCAN_DONE||e.kind==PM_BT_STOPPED){
        /* An intermediate stop is not completion of discover/connect. */
        if(e.kind==PM_BT_STOPPED&&bt_action!=1)return;
        bt_waiting=false;
        (void)bt_report("{\"event\":\"complete\",\"success\":true}");
    }else if(e.kind==PM_BT_ERROR){pm_logf("bt runtime error generation=%llu status=0x%04x d1=%ld d2=%ld d3=%ld\n",(unsigned long long)e.generation,e.status,(long)e.detail1,(long)e.detail2,(long)e.detail3);bt_waiting=false;if(!bt_input.connected)(void)bt_report("{\"event\":\"complete\",\"success\":false}");
    }else if(e.kind==PM_BT_DISCONNECTED){
        pm_bluetooth_input_reset(&bt_input,e.generation,false);(void)bt_report("{\"event\":\"disconnected\"}");
    }else if(e.kind==PM_BT_INPUT){
        /* AVRCP can reconnect while A2DP and its command generation survive. */
        if(bt_input_epoch!=e.input_epoch){
            bt_input_epoch=e.input_epoch;
            pm_bluetooth_input_reset(&bt_input,e.generation,bt_input.connected);
        }
        pm_media_operation op;if(pm_bluetooth_input_event(&bt_input,e.generation,e.operation,!e.pressed,e.input_ms,to_ms_since_boot(get_absolute_time()),&op))(void)send_media_input(op);
    }
}
#else
static void poll_bluetooth(void){}
#endif
