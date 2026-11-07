/* BT-AUDIO-20261107: bounded main-loop PCM wire bridge. */
#if PM_BLUETOOTH_AUDIO
static struct {
    uint64_t inc,session,display,stream,first,accepted,consumed,final_frame;
    uint32_t started_ms,report_ms,diagnostic_ms;
    bool active,playing,failed,dirty,finishing;
} bt_media;
static int16_t bt_media_pcm[512];
static bool bt_media_current(void){
    return state==ACTIVE&&bt_media.inc==incarnation&&bt_media.session==session_epoch&&bt_media.display==display_epoch;
}
static void bt_media_log_runtime(void){
    pm_bt_audio_diagnostics d={0};pm_bt_link_snapshot link={0};
    cyw43_arch_lwip_begin();if(bt_initialized){pm_bt_runtime_audio_diagnostics(&d);pm_bt_runtime_link_snapshot(&link);}cyw43_arch_lwip_end();
    pm_logf("bt audio acl handle=%u role=%u mode=%u packet_known=%u packets=0x%04x interval_known=%u interval_slots=%u\n",link.handle,link.role,link.mode,link.packet_types_known,link.packet_types,link.interval_known,link.mode_interval);
    pm_logf("bt audio packets n=%lu frames=%lu last_n=%lu bytes=%lu mtu=%lu bitpool=%lu\n",(unsigned long)d.packets,(unsigned long)d.frames,(unsigned long)d.last_count,(unsigned long)d.last_bytes,(unsigned long)d.mtu,(unsigned long)d.bitpool);
    pm_logf("bt audio timing can_total=%lu can_max=%lu timer_total=%lu timer_max=%lu encode_total=%lu encode_max=%lu\n",(unsigned long)d.can_wait_total_ms,(unsigned long)d.can_wait_max_ms,(unsigned long)d.timer_late_total_ms,(unsigned long)d.timer_late_max_ms,(unsigned long)d.encode_total_ms,(unsigned long)d.encode_max_ms);
}
static void bt_media_end(bool failed){
    cyw43_arch_lwip_begin();if(bt_initialized)pm_bt_runtime_audio_end();cyw43_arch_lwip_end();
    pm_logf("bt audio end stream=%llu accepted=%llu consumed=%llu failed=%u eof=%u final=%llu\n",
        (unsigned long long)bt_media.stream,(unsigned long long)bt_media.accepted,
        (unsigned long long)bt_media.consumed,failed,bt_media.finishing,(unsigned long long)bt_media.final_frame);
    bt_media_log_runtime();
    bt_media.active=false;bt_media.playing=false;bt_media.failed=failed;bt_media.dirty=true;
}
static pm_api_result api_audio(uint8_t kind,const char *j){
    uint64_t inc,session,display,stream,first,rate;
    if(!number(j,"incarnation",&inc)||!number(j,"session_epoch",&session)||
       !number(j,"display_epoch",&display)||!number(j,"stream_id",&stream)||!stream)return PM_API_REJECTED;
    if(state!=ACTIVE||inc!=incarnation||session!=session_epoch||display!=display_epoch){
        /* Once per rejected identity, not per PCM packet or polling turn. */
        static uint64_t logged_inc,logged_session,logged_display,logged_stream;
        if(kind==22&&(inc!=logged_inc||session!=logged_session||display!=logged_display||stream!=logged_stream)){
            logged_inc=inc;logged_session=session;logged_display=display;logged_stream=stream;
            pm_logf("bt audio ignored stream=%llu requested_inc=%llu current_inc=%llu requested_session=%llu current_session=%llu requested_display=%llu current_display=%llu state=%u\n",
                (unsigned long long)stream,(unsigned long long)inc,(unsigned long long)incarnation,
                (unsigned long long)session,(unsigned long long)session_epoch,
                (unsigned long long)display,(unsigned long long)display_epoch,(unsigned)state);
        }
        return PM_API_IGNORED;
    }
    if(kind==26){
        uint64_t end;
        if(inc!=bt_media.inc||session!=bt_media.session||display!=bt_media.display||stream!=bt_media.stream||!bt_media.active)return PM_API_IGNORED;
        if(!number(j,"final_frame",&end)||end<bt_media.accepted||end<bt_media.first||(end-bt_media.first)%128)return PM_API_REJECTED;
        if(bt_media.finishing&&end!=bt_media.final_frame)return PM_API_REJECTED;
        cyw43_arch_lwip_begin();bool finish_ok=pm_bt_runtime_audio_finish(stream,end);cyw43_arch_lwip_end();
        if(!finish_ok)return PM_API_REJECTED;
        bt_media.finishing=true;bt_media.final_frame=end;return PM_API_PENDING;
    }
    if(kind==23){
        if(inc==bt_media.inc&&session==bt_media.session&&display==bt_media.display&&stream==bt_media.stream)bt_media_end(false);
        return PM_API_COMPLETED;
    }
    if(kind!=22||!number(j,"first_frame",&first)||!number(j,"rate",&rate)||rate!=48000)return PM_API_REJECTED;
    if(bt_media.inc==inc&&bt_media.session==session&&stream<=bt_media.stream)return PM_API_IGNORED;
    if(bt_media.active)bt_media_end(false);
    memset(&bt_media,0,sizeof bt_media);
    bt_media.inc=inc;bt_media.session=session;bt_media.display=display;bt_media.stream=stream;
    bt_media.first=bt_media.accepted=bt_media.consumed=first;
    bt_media.started_ms=to_ms_since_boot(get_absolute_time());bt_media.dirty=true;
    cyw43_arch_lwip_begin();
    uint32_t negotiated=bt_initialized?pm_bt_runtime_rate():0;
    bool was_active=bt_initialized&&pm_bt_runtime_audio_active();
    bool ok=bt_initialized&&negotiated==rate&&pm_bt_runtime_audio_begin(stream,first);
    cyw43_arch_lwip_end();
    pm_logf("bt audio begin stream=%llu rate=%lu negotiated=%lu init=%u prior_active=%u ok=%u first=%llu\n",
        (unsigned long long)stream,(unsigned long)rate,(unsigned long)negotiated,bt_initialized,was_active,ok,(unsigned long long)first);
    bt_media.active=ok;bt_media.failed=!ok;
    return ok?PM_API_PENDING:PM_API_REJECTED;
}
static uint64_t bt_media_be64(const uint8_t *p){uint64_t v=0;for(unsigned i=0;i<8;i++)v=(v<<8)|p[i];return v;}
static void process_audio_bytes(const uint8_t *p,size_t n){
    if(!bt_media.active||!bt_media_current()||n<44||n>1064||(n-40)%4)return;
    if(bt_media_be64(p)!=bt_media.inc||bt_media_be64(p+8)!=bt_media.session||
       bt_media_be64(p+16)!=bt_media.display||bt_media_be64(p+24)!=bt_media.stream)return;
    uint64_t first=bt_media_be64(p+32);size_t frames=(n-40)/4;
    /* Old retransmissions never become a new discontinuity. */
    if(first<bt_media.accepted)return;
    if(first>UINT64_MAX-frames||(bt_media.finishing&&first+frames>bt_media.final_frame)){bt_media_end(true);return;}
    for(size_t i=0;i<frames*2;i++)bt_media_pcm[i]=(int16_t)((uint16_t)p[40+i*2]|((uint16_t)p[41+i*2]<<8));
    cyw43_arch_lwip_begin();
    pm_bluetooth_audio_result r=pm_bt_runtime_audio_push(bt_media.stream,first,bt_media_pcm,frames);
    if(r==PM_BT_PCM_OK)bt_media.accepted=pm_bt_runtime_audio_next_frame();
    cyw43_arch_lwip_end();
    if(r==PM_BT_PCM_OK&&first==bt_media.first)
        pm_logf("bt audio first stream=%llu frames=%u accepted=%llu\n",(unsigned long long)bt_media.stream,(unsigned)frames,(unsigned long long)bt_media.accepted);
    if(r!=PM_BT_PCM_OK){pm_logf("bt audio push_error stream=%llu result=%u first=%llu frames=%u\n",(unsigned long long)bt_media.stream,(unsigned)r,(unsigned long long)first,(unsigned)frames);bt_media_end(true);}
    bt_media.dirty=true;
}
static void poll_audio(void){
    if(!bt_media.stream)return;
    if(!bt_media_current()){if(bt_media.active)bt_media_end(false);bt_media.dirty=false;return;}
    uint32_t now=to_ms_since_boot(get_absolute_time());
    if(bt_media.active){
        cyw43_arch_lwip_begin();
        bt_media.consumed=pm_bt_runtime_audio_consumed_frames();
        bool alive=pm_bt_runtime_audio_active();
        uint8_t status=0;bool requested_play=false;
        uint64_t buffered=bt_media.accepted-bt_media.consumed;
        if(alive&&!bt_media.playing&&(buffered>=PM_BT_PCM_FRAMES||(buffered>=128&&bt_media.finishing&&bt_media.accepted==bt_media.final_frame))){
            requested_play=true;status=pm_bt_runtime_play();if(!status)bt_media.playing=true;
        }
        cyw43_arch_lwip_end();
        if(requested_play)pm_logf("bt audio play stream=%llu status=%u buffered=%llu\n",(unsigned long long)bt_media.stream,status,(unsigned long long)buffered);
        if(now-bt_media.diagnostic_ms>=1000){
            bt_media.diagnostic_ms=now;bt_media_log_runtime();
            pm_logf("bt audio progress stream=%llu accepted=%llu consumed=%llu alive=%u playing=%u\n",(unsigned long long)bt_media.stream,(unsigned long long)bt_media.accepted,(unsigned long long)bt_media.consumed,alive,bt_media.playing);
        }
        if(!alive||status)bt_media_end(!(bt_media.finishing&&bt_media.consumed>=bt_media.final_frame));
        else if(bt_media.finishing&&bt_media.final_frame==bt_media.first)bt_media_end(false);
    }
    if(!bt_media.dirty&&(!bt_media.active||now-bt_media.report_ms<10))return;
    if(now-bt_media.report_ms<10)return;
    int n=snprintf(outbound_json,sizeof outbound_json,
      "{\"type\":\"audio_status\",\"data\":{\"incarnation\":%llu,\"session_epoch\":%llu,\"display_epoch\":%llu,\"stream_id\":%llu,\"accepted\":%llu,\"consumed\":%llu,\"failed\":%s}}",
      (unsigned long long)bt_media.inc,(unsigned long long)bt_media.session,(unsigned long long)bt_media.display,
      (unsigned long long)bt_media.stream,(unsigned long long)bt_media.accepted,(unsigned long long)bt_media.consumed,bt_media.failed?"true":"false");
    if(n>0&&(size_t)n<sizeof outbound_json&&pm_net_send(25,outbound_json)){bt_media.report_ms=now;bt_media.dirty=false;}
}
#else
static pm_api_result api_audio(uint8_t kind,const char *j){(void)kind;(void)j;return PM_API_IGNORED;}
static void process_audio_bytes(const uint8_t *p,size_t n){(void)p;(void)n;}
static void poll_audio(void){}
#endif
