#include "trace.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "pico/stdlib.h"
#include "pico/cyw43_arch.h"
#include "pico/unique_id.h"
#include "pico/stdio_usb.h"
#include "hardware/flash.h"
#include "hardware/sync.h"
#include "hardware/regs/addressmap.h"
#include "core.h"
#include "display.h"
#include "net.h"
#include "video.h"

#if defined(PM_NORMAL_BUILD) && PM_NORMAL_BUILD
// Release selection is a build property. Keep the private device config intact.
#undef PM_DIAGNOSTIC_BOOT
#undef PM_DIAGNOSTIC_STAGE
#define PM_DIAGNOSTIC_BOOT 0
#define PM_DIAGNOSTIC_STAGE 0
#endif

typedef enum { WIFI, SERVER, SESSION, ACTIVE, MENU, STANDBY, RETRY } app_state;
static const pm_profile profiles[2]={
    {PM_HOME_SSID,PM_HOME_PASSWORD,PM_HOME_SERVER_IP,false,NULL,NULL,NULL,0},
    {PM_AWAY_SSID,PM_AWAY_PASSWORD,PM_AWAY_SERVER_IP,true,
     PM_AWAY_PROXY_IP,PM_AWAY_PROXY_USER,PM_AWAY_PROXY_PASSWORD,PM_AWAY_PROXY_PORT}
};
static struct {
    uint32_t magic, profile;
    uint64_t incarnation;
    uint64_t revision;
    uint32_t wifi_s,app_s,retry_s,retry_max_s,heartbeat_s,offline_s,long_ms;
    uint32_t crc;
} settings;
static app_state state;
static pm_button buttons[4];
#if PM_BUTTON_COUNT == 4
static const uint button_pins[4]={PM_BUTTON_SUB_PIN,PM_BUTTON_UP_PIN,PM_BUTTON_POWER_PIN,PM_BUTTON_DOWN_PIN};
#elif PM_BUTTON_COUNT == 2
static const uint button_pins[4]={PM_BUTTON_UP_PIN,PM_BUTTON_DOWN_PIN};
#elif PM_BUTTON_COUNT == 0
static const uint button_pins[4]={0};
#else
#error "Supported input counts are 0, 2 and 4"
#endif
static unsigned input_count(void){return PM_BUTTON_COUNT;}
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
static bool drawing,panel_standby;
static pm_token drawing_token;
#endif
static uint8_t active_profile,menu_profile;
static uint64_t owner_epoch=1,incarnation,session_epoch,display_epoch;
static uint32_t started_ms,retry_ms=5000,last_heartbeat_ms;
static bool wifi_enabled,await_selection,sub_mode,have_display,pending_standby,pending_sub_cancel;
static pm_token last_rendered;
static pm_token pending_render_token;
static bool pending_render_ack;
static pm_assembly assembly;
static char json[18000];
_Static_assert(sizeof json>4u*PM_FRAGMENT_BYTES+1024u,"legacy JSON chunk capacity");
#if PM_DISPLAY_TYPE == PM_DISPLAY_LCD
static pm_video video;
static bool receiving_stream;
static uint64_t receiving_sequence,last_video_status_us;
#endif
// Outbound messages are serialized on the main loop, including the TCP copy.
static char outbound_json[768];
static uint8_t fragment[PM_FRAGMENT_BYTES];

static uint32_t now_ms(void){return to_ms_since_boot(get_absolute_time());}
static const char *profile_name(void){return active_profile?"AWAY":"HOME";}
static const char *link_name(int link){
    switch(link){
    case CYW43_LINK_DOWN:return "DOWN";case CYW43_LINK_JOIN:return "JOIN";
    case CYW43_LINK_NOIP:return "NOIP";case CYW43_LINK_UP:return "UP";
    case CYW43_LINK_FAIL:return "FAIL";case CYW43_LINK_NONET:return "NONET";
    case CYW43_LINK_BADAUTH:return "BADAUTH";default:return "UNKNOWN";
    }
}
static int logged_wifi_link=99,logged_tcpip_link=99;
static const char *app_name(void){
    switch(state){case WIFI:return "WIFI";case SERVER:return "SERVER";
    case SESSION:return "SESSION";case ACTIVE:return "ACTIVE";case MENU:return "MENU";
    case STANDBY:return "STANDBY";case RETRY:return "RETRY";default:return "UNKNOWN";}
}
static const char *transport_name(void){
    switch(pm_net_state()){case PM_NET_DOWN:return "DOWN";case PM_NET_TCP:return "TCP";
    case PM_NET_SOCKS:return "SOCKS";case PM_NET_WS:return "WS";case PM_NET_READY:return "READY";
    case PM_NET_FAILED:return "FAILED";default:return "UNKNOWN";}
}
static void log_usb_snapshot(void){
    static bool was_connected;
    static uint32_t last_ms;
    bool connected=stdio_usb_connected();
    if(!connected){was_connected=false;return;}
    uint32_t now=now_ms();
    if(was_connected && now-last_ms<15000)return;
    was_connected=true;last_ms=now;
    int wifi=wifi_enabled?cyw43_wifi_link_status(&cyw43_state,CYW43_ITF_STA):CYW43_LINK_DOWN;
    int tcpip=wifi_enabled?cyw43_tcpip_link_status(&cyw43_state,CYW43_ITF_STA):CYW43_LINK_DOWN;
    pm_logf("net status profile=%s app=%s transport=%s wifi=%s tcpip=%s ip_assigned=%u\n",
        profile_name(),app_name(),transport_name(),link_name(wifi),link_name(tcpip),
        tcpip==CYW43_LINK_UP?1u:0u);
}
static void log_wifi_progress(void){
    int wifi=cyw43_wifi_link_status(&cyw43_state,CYW43_ITF_STA);
    int tcpip=cyw43_tcpip_link_status(&cyw43_state,CYW43_ITF_STA);
    if(wifi==logged_wifi_link && tcpip==logged_tcpip_link)return;
    logged_wifi_link=wifi;logged_tcpip_link=tcpip;
    pm_logf("net profile=%s wifi=%s tcpip=%s ip_assigned=%u\n",
        profile_name(),link_name(wifi),link_name(tcpip),tcpip==CYW43_LINK_UP?1u:0u);
}
static bool profile_ready(unsigned index){
    const pm_profile *p=&profiles[index];
    return p->ssid && p->ssid[0] && p->password && p->password[0] &&
        p->server_ip && p->server_ip[0] &&
        (!p->socks || (p->proxy_ip && p->proxy_ip[0] && p->proxy_port &&
                       p->proxy_user && p->proxy_user[0] &&
                       p->proxy_password && p->proxy_password[0]));
}
static void defaults(void) {
    memset(&settings,0,sizeof settings);settings.magic=0x504d4431;
    settings.wifi_s=15;settings.app_s=10;settings.retry_s=5;
    settings.retry_max_s=30;settings.heartbeat_s=15;
    settings.offline_s=45;settings.long_ms=1800;
}
static void load_settings(void) {
    defaults();
    const uint8_t *p=(const uint8_t *)(XIP_BASE+PICO_FLASH_SIZE_BYTES-FLASH_SECTOR_SIZE);
    uint32_t *magic=(uint32_t *)(void *)p;
    if(*magic!=settings.magic)return;
    typeof(settings) copy;memcpy(&copy,p,sizeof copy);
    uint32_t crc=pm_crc32(p,offsetof(typeof(settings),crc));
    if(copy.crc!=crc || copy.profile>1 || copy.wifi_s<5 || copy.wifi_s>120 ||
       copy.app_s<3 || copy.app_s>120 || copy.retry_s<1 || copy.retry_s>60 ||
       copy.retry_max_s<copy.retry_s || copy.retry_max_s>300 ||
       copy.heartbeat_s<5 || copy.heartbeat_s>120 ||
       copy.offline_s<2*copy.heartbeat_s || copy.offline_s>600 ||
       copy.long_ms<1500 || copy.long_ms>2000)return;
    settings=copy;
}
static void store_settings(void) {
    static uint8_t page[FLASH_PAGE_SIZE] __attribute__((aligned(4)));
    settings.crc=pm_crc32((const uint8_t *)&settings,offsetof(typeof(settings),crc));
    memset(page,0xff,sizeof page);memcpy(page,&settings,sizeof settings);
    uint32_t irq=save_and_disable_interrupts();
    flash_range_erase(PICO_FLASH_SIZE_BYTES-FLASH_SECTOR_SIZE,FLASH_SECTOR_SIZE);
    flash_range_program(PICO_FLASH_SIZE_BYTES-FLASH_SECTOR_SIZE,page,FLASH_PAGE_SIZE);
    restore_interrupts(irq);
}
static void local(const char *one,const char *two) {
    owner_epoch++;pm_reset(&assembly);have_display=false;pending_render_ack=false;
    pm_display_wake();pm_display_status(one,two);
#if PM_DIAGNOSTIC_BOOT || PM_DIAGNOSTIC_STAGE
    pm_logf("lcd local: %s / %s\n",one,two);
#endif
}
static void wifi_off(void){
    pm_net_close();
    if(wifi_enabled){cyw43_arch_disable_sta_mode();wifi_enabled=false;}
}
static void standby(void){
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
    pm_display_standby();
    if(pm_display_busy()){
        if(!panel_standby)owner_epoch++;
        panel_standby=true;pm_reset(&assembly);pending_render_ack=false;
        pending_standby=false;wifi_off();return;
    }
#endif
    pm_display_standby();
#if PM_DIAGNOSTIC_BOOT
    pm_logf("%s\n","lcd black: standby");
#endif
    owner_epoch++;pm_reset(&assembly);have_display=false;pending_render_ack=false;
    wifi_off();pending_standby=false;state=STANDBY;
}
static void start_wifi(void){
    pm_net_close();
    if(!wifi_enabled){
        cyw43_arch_enable_sta_mode();wifi_enabled=true;
        // Match the working YouTube player: PM2 is the SDK default even in
        // performance mode. Streaming/control use an awake radio; standby
        // still disables STA. Change no profile or connection credentials.
        cyw43_arch_lwip_begin();
        int result=cyw43_wifi_pm(&cyw43_state,CYW43_NONE_PM);
        cyw43_arch_lwip_end();
        pm_logf("net wifi_pm requested=none sdk_result=%d\n",result);
    }
    incarnation=0;session_epoch=0;display_epoch=0;
    await_selection=false;sub_mode=false;pending_standby=false;pending_sub_cancel=false;
    local("WIFI", "CONNECTING");
    const pm_profile *p=&profiles[active_profile];
    logged_wifi_link=logged_tcpip_link=99;
    if(!profile_ready(active_profile)){
        pm_logf("net profile=%s wifi_start config_ready=0\n",profile_name());
        state=RETRY;started_ms=now_ms();return;
    }
    int result=cyw43_arch_wifi_connect_async(p->ssid,p->password,CYW43_AUTH_WPA2_AES_PSK);
    pm_logf("net profile=%s wifi_start sdk_result=%d\n",profile_name(),result);
    log_wifi_progress();
    if(result){
        state=RETRY;started_ms=now_ms();return;
    }
    state=WIFI;started_ms=now_ms();
}
static void start_server(void){
    incarnation=0;session_epoch=0;display_epoch=0;
    await_selection=false;sub_mode=false;pending_standby=false;pending_sub_cancel=false;
    local("SERVER","CONNECTING");pm_net_begin(&profiles[active_profile]);
    state=SERVER;started_ms=now_ms();
}
static void retry(void){
    bool wifi_was_up=wifi_enabled &&
        cyw43_tcpip_link_status(&cyw43_state,CYW43_ITF_STA)==CYW43_LINK_UP;
    log_wifi_progress();
    pm_logf("net profile=%s retry app=%s transport=%s delay_ms=%lu\n",
        profile_name(),app_name(),transport_name(),(unsigned long)retry_ms);
    // A failed server/proxy session does not invalidate a healthy Wi-Fi/IP link.
    if(wifi_was_up)pm_net_close();else wifi_off();
    local(wifi_was_up?"SERVER":"WIFI",wifi_was_up?"CONNECTING":"RETRY");
    state=RETRY;started_ms=now_ms();
}
static void menu(void){
    wifi_off();menu_profile=active_profile;state=MENU;
    pm_logf("net profile=%s menu_open\n",profile_name());
    local("NETWORK",menu_profile?"AWAY":"HOME");
}
static bool number(const char *object,const char *key,uint64_t *out) {
    char name[64];snprintf(name,sizeof name,"\"%s\"",key);
    const char *p=strstr(object,name);if(!p)return false;
    p+=strlen(name);while(*p==' '||*p=='\t')p++;
    if(*p++!=':')return false;
    while(*p==' '||*p=='\t')p++;
    if(*p<'0'||*p>'9')return false;
    char *end;unsigned long long v=strtoull(p,&end,10);
    if(end==p || (*end!=',' && *end!='}' && *end!=']'))return false;
    *out=v;return true;
}
static bool string_equals(const char *object,const char *key,const char *expected){
    char name[64];snprintf(name,sizeof name,"\"%s\"",key);
    const char *p=strstr(object,name);if(!p)return false;
    p=strchr(p,':');if(!p)return false;p++;
    while(*p==' ')p++;
    if(*p++!='\"')return false;
    size_t n=strlen(expected);return !strncmp(p,expected,n) && p[n]=='\"';
}
static bool token_from(const char *j,pm_token *t) {
    return string_equals(j,"device_id",PM_DEVICE_ID) &&
        number(j,"incarnation",&t->incarnation) &&
        number(j,"session_epoch",&t->session_epoch) &&
        number(j,"display_epoch",&t->display_epoch) &&
        number(j,"screen_owner_epoch",&t->screen_owner_epoch) &&
        number(j,"frame_id",&t->frame_id);
}
static bool send_simple(uint8_t kind,const char *type){
    
    int n=snprintf(outbound_json,sizeof outbound_json,
        "{\"type\":\"%s\",\"data\":{\"incarnation\":%llu,\"session_epoch\":%llu}}",
        type,(unsigned long long)incarnation,(unsigned long long)session_epoch);
    return n>0 && (size_t)n<sizeof outbound_json && pm_net_send(kind,outbound_json);
}
static bool send_ack(uint8_t kind,const char *type,pm_token t){
    int n=snprintf(outbound_json,sizeof outbound_json,
        "{\"type\":\"%s\",\"data\":{\"token\":{\"device_id\":\"%s\","
        "\"incarnation\":%llu,\"session_epoch\":%llu,\"display_epoch\":%llu,"
        "\"screen_owner_epoch\":%llu,\"frame_id\":%llu}}}",type,PM_DEVICE_ID,
        (unsigned long long)t.incarnation,(unsigned long long)t.session_epoch,
        (unsigned long long)t.display_epoch,(unsigned long long)t.screen_owner_epoch,
        (unsigned long long)t.frame_id);
    pm_trace_kind before=kind==8?PM_TRACE_RECEIPT_BEGIN:PM_TRACE_RENDER_BEGIN;
    pm_trace_kind after=kind==8?PM_TRACE_RECEIPT_END:PM_TRACE_RENDER_END;
    pm_trace_frame_context(t.frame_id);
    pm_trace_event(before,t.frame_id,kind,0);
    bool ok=n>0 && (size_t)n<sizeof outbound_json && pm_net_send(kind,outbound_json);
    pm_trace_event(after,t.frame_id,ok?1:0,0);
    pm_trace_frame_context(0);return ok;
}
static bool send_pending_render_ack(void){
    if(state!=ACTIVE || !pending_render_ack)return true;
    pm_token t=pending_render_token;pending_render_ack=false;
    if(!pm_token_current(t,incarnation,session_epoch,display_epoch,owner_epoch))return true;
    return send_ack(9,"rendered_ack",t);
}
#if PM_DISPLAY_TYPE == PM_DISPLAY_LCD
// Functional deadlines must advance even when diagnostic tracing is disabled.
static uint64_t video_now_us(void){return time_us_64();}
static bool video_mode(void){return video.active;}
static void poll_video(void){
    if(state!=ACTIVE || !video.active)return;
    if(await_selection || pending_standby){video.ready=false;return;}
    if(!pm_token_current(video.context,incarnation,session_epoch,display_epoch,owner_epoch)){
        pm_video_reset(&video);return;
    }
    uint64_t now=video_now_us();uint8_t *pixels=pm_video_begin_draw(&video,now);
    if(pixels){
        pm_trace_frame_context(video.token.frame_id);
        bool ok=pm_display_frame(pixels,PM_IMAGE_BYTES);pm_trace_frame_context(0);
        if(!ok){pm_video_reset(&video);retry();return;}
        pm_video_finish_draw(&video,video_now_us());have_display=true;last_rendered=video.last_rendered;
    }
    now=video_now_us();
    if(!video.rendered || now-last_video_status_us<1000000u)return;
    pm_token t=video.last_rendered;
    int n=snprintf(outbound_json,sizeof outbound_json,
        "{\"type\":\"stream_status\",\"data\":{\"token\":{\"device_id\":\"%s\","
        "\"incarnation\":%llu,\"session_epoch\":%llu,\"display_epoch\":%llu,\"screen_owner_epoch\":%llu,\"frame_id\":%llu},"
        "\"generation\":%llu,\"received_frames\":%llu,\"rendered_frames\":%llu,\"dropped_frames\":%llu,\"elapsed_ms\":%llu,\"max_gap_us\":%llu}}",
        PM_DEVICE_ID,(unsigned long long)t.incarnation,(unsigned long long)t.session_epoch,
        (unsigned long long)t.display_epoch,(unsigned long long)t.screen_owner_epoch,(unsigned long long)t.frame_id,
        (unsigned long long)video.generation,(unsigned long long)video.received,(unsigned long long)video.rendered,
        (unsigned long long)video.dropped,(unsigned long long)((now-video.first_us)/1000),(unsigned long long)video.max_gap_us);
    if(n>0 && (size_t)n<sizeof outbound_json)pm_net_send_optional(19,outbound_json);
    last_video_status_us=now;
}
#else
static bool video_mode(void){return false;}
static void poll_video(void){}
#endif
static bool send_hello(void){
    int n=snprintf(outbound_json,sizeof outbound_json,
        "{\"type\":\"hello\",\"data\":{\"device_id\":\"%s\","
        "\"capabilities\":[\"%s\",\"buttons%u\",\"frame-bytes-v1\"%s],\"screen_owner_epoch\":%llu}}",
        PM_DEVICE_ID,pm_display_capability(),PM_BUTTON_COUNT,
        PM_DISPLAY_TYPE==PM_DISPLAY_LCD?",\"lcd-push-v1\"":"",(unsigned long long)owner_epoch);
    return n>0 && (size_t)n<sizeof outbound_json && pm_net_send(1,outbound_json);
}
static void heartbeat(void){
    int n=snprintf(outbound_json,sizeof outbound_json,
        "{\"type\":\"heartbeat\",\"data\":{\"incarnation\":%llu,"
        "\"session_epoch\":%llu,\"applied_revision\":%llu}}",
        (unsigned long long)incarnation,(unsigned long long)session_epoch,
        (unsigned long long)settings.revision);
    if(n>0&&(size_t)n<sizeof outbound_json)pm_net_send(5,outbound_json);
#if PM_DIAGNOSTIC_STAGE
    pm_logf("%s\n","diag heartbeat_sent");
#endif
}
static bool parse_bytes(const char *j,size_t *count){
    const char *p=strstr(j,"\"bytes\"");if(!p)return false;
    p=strchr(p,':');if(!p)return false;p++;
    while(*p==' ')p++;
    if(*p++!='[')return false;
    *count=0;
    while(true) {
        while(*p==' ')p++;
        if(*p==']')return *count>0;
        if(*count>=PM_FRAGMENT_BYTES || *p<'0'||*p>'9')return false;
        char *end;unsigned long n=strtoul(p,&end,10);
        if(n>255)return false;
        fragment[(*count)++]=(uint8_t)n;p=end;
        if(*p==',')p++;
        else if(*p!=']')return false;
    }
}
static bool settings_message(const char *j){
    uint64_t v[9];const char *keys[]={"incarnation","revision","wifi_timeout_seconds",
        "app_timeout_seconds","retry_initial_seconds","retry_max_seconds",
        "heartbeat_seconds","offline_seconds","two_button_long_press_millis"};
    for(unsigned i=0;i<9;i++)if(!number(j,keys[i],&v[i]))return false;
    if(v[0]!=incarnation || (settings.incarnation==incarnation && v[1]<settings.revision) ||
       v[2]<5 || v[2]>120 ||
       v[3]<3 || v[3]>120 || v[4]<1 || v[4]>60 || v[5]<v[4] || v[5]>300 ||
       v[6]<5 || v[6]>120 || v[7]<2*v[6] || v[7]>600 ||
       v[8]<1500 || v[8]>2000)return false;
    settings.incarnation=incarnation;settings.revision=v[1];settings.wifi_s=v[2];settings.app_s=v[3];
    settings.retry_s=v[4];settings.retry_max_s=v[5];
    settings.heartbeat_s=v[6];settings.offline_s=v[7];settings.long_ms=v[8];
    retry_ms=settings.retry_s*1000u;
    store_settings();
    int n=snprintf(outbound_json,sizeof outbound_json,
        "{\"type\":\"settings_stored\",\"data\":{\"incarnation\":%llu,\"revision\":%llu}}",
        (unsigned long long)incarnation,(unsigned long long)settings.revision);
    return n>0&&(size_t)n<sizeof outbound_json && pm_net_send(4,outbound_json);
}
static void receive_chunk(pm_token t,uint64_t offset,const uint8_t *bytes,size_t count){
    if(state!=ACTIVE || await_selection || !pm_token_current(t,incarnation,session_epoch,display_epoch,owner_epoch) ||
       !assembly.active || !pm_token_equal(assembly.token,t))return;
    bool complete;
    if(!pm_chunk(&assembly,t,offset,bytes,count,&complete))return;
    if(!complete)return;
    pm_trace_event(PM_TRACE_CRC_OK,t.frame_id,0,PM_IMAGE_BYTES);
    if(!pm_token_current(t,incarnation,session_epoch,display_epoch,owner_epoch))return;
#if PM_DISPLAY_TYPE == PM_DISPLAY_LCD
    if(receiving_stream){
        pm_video_offer(&video,t,receiving_sequence,assembly.pixels,PM_IMAGE_BYTES);return;
    }
#endif
    if(!send_ack(8,"receipt_ack",t)){retry();return;}
    if(!have_display || !pm_token_equal(last_rendered,t)) {
#if PM_DIAGNOSTIC_BOOT
        size_t nonzero=0;
        for(size_t i=0;i<PM_IMAGE_BYTES;i++)if(assembly.pixels[i])nonzero++;
        pm_logf("frame %llu: nonzero bytes %u/%u\n",
               (unsigned long long)t.frame_id,(unsigned)nonzero,(unsigned)PM_IMAGE_BYTES);
        pm_display_reference_pattern();sleep_ms(500);
#endif
        pm_trace_frame_context(t.frame_id);
        bool drawn=pm_display_frame(assembly.pixels,PM_IMAGE_BYTES);
        pm_trace_frame_context(0);
        if(!drawn){retry();return;}
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
        drawing=true;drawing_token=t;return;
#else
        have_display=true;last_rendered=t;
#endif
#if PM_DIAGNOSTIC_STAGE
        pm_logf("diag frame_spi_complete=%llu\n",(unsigned long long)t.frame_id);
#endif
#if PM_DIAGNOSTIC_BOOT
        pm_logf("lcd SPI complete: frame %llu\n",(unsigned long long)t.frame_id);
#endif
    }
    pending_render_token=t;pending_render_ack=true;
}
static uint64_t binary_u64(const uint8_t *p){uint64_t v=0;for(unsigned i=0;i<8;i++)v=(v<<8)|p[i];return v;}
static void process_frame_bytes(const uint8_t *p,size_t length){
    if(!length)return;
    size_t id_length=p[0],header=1+id_length+44;
    if(id_length!=strlen(PM_DEVICE_ID) || length<=header || length-header>PM_FRAGMENT_BYTES ||
       memcmp(p+1,PM_DEVICE_ID,id_length))return;
    const uint8_t *fields=p+1+id_length;
    pm_token t={.incarnation=binary_u64(fields),.session_epoch=binary_u64(fields+8),
        .display_epoch=binary_u64(fields+16),.screen_owner_epoch=binary_u64(fields+24),.frame_id=binary_u64(fields+32)};
    uint32_t offset=(uint32_t)fields[40]<<24|(uint32_t)fields[41]<<16|(uint32_t)fields[42]<<8|fields[43];
    receive_chunk(t,offset,p+header,length-header);
}
static void process(uint8_t kind,const char *j){
    const char *type=NULL;
    switch(kind){
    case 2:type="welcome";break;case 3:type="settings";break;
    case 10:type="input_accepted";break;case 11:type="error";break;
    case 12:type="frame_start";break;case 13:type="frame_chunk";break;
    case 17:type="plugin_position";break;default:break;
    }
    if(!type || !string_equals(j,"type",type)){retry();return;}
    if(kind==2 && state==SESSION && string_equals(j,"type","welcome")){
        if(!number(j,"incarnation",&incarnation)||!number(j,"session_epoch",&session_epoch)){
            retry();return;
        }
        if(settings.incarnation!=incarnation){
            uint32_t selected=settings.profile;defaults();settings.profile=selected;
        }
        retry_ms=settings.retry_s*1000u;
        display_epoch=0;state=ACTIVE;last_heartbeat_ms=now_ms();
#if PM_DIAGNOSTIC_STAGE
        pm_logf("diag stage=welcome session=%llu\n",(unsigned long long)session_epoch);
#endif
        pm_display_status("PICO MADO","WAIT IMAGE");return;
    }
    if(state!=ACTIVE)return;
    if(kind==3){if(!settings_message(j))retry();return;}
    if(kind==10 || kind==17){
        uint64_t epoch=display_epoch,reply_inc,reply_session;
        bool contextual=strstr(j,"\"input_context\"")!=NULL;
        bool has_epoch=number(j,"display_epoch",&epoch);
        if((contextual && (!has_epoch || !number(j,"incarnation",&reply_inc) ||
             !number(j,"session_epoch",&reply_session) || reply_inc!=incarnation || reply_session!=session_epoch)) ||
           (kind==10 && !has_epoch) || (has_epoch && epoch<display_epoch)){
            pm_logf("net reply kind=%u current=%llu result=stale\n",kind,(unsigned long long)display_epoch);return;
        }
        // A matching reply may arrive after FrameStart or between its chunks.
        // Only a genuinely newer generation invalidates an assembly.
        bool keep=assembly.active && (!has_epoch || epoch==display_epoch);
        if(has_epoch && epoch>display_epoch){display_epoch=epoch;pm_reset(&assembly);}
        await_selection=false;
        if(string_equals(j,"mode","normal"))sub_mode=false;
        else if(string_equals(j,"mode","sub"))sub_mode=true;
        else if(kind==17)sub_mode=!pending_sub_cancel;
        else if(pending_sub_cancel)sub_mode=false;
        pending_sub_cancel=false;
        pm_logf("net reply kind=%u epoch=%llu keep=%u\n",kind,(unsigned long long)display_epoch,keep?1u:0u);
        if(kind==10 && pending_standby)standby();
        return;
    }
    if(kind==11){
        // Only fixed protocol error names are printable; never echo received JSON.
        static const char *const codes[]={"stale_frame","invalid_ack","invalid_ack_token","invalid_input",
            "invalid_frame","invalid_hello","stale_session","stale_settings",
            "unavailable","hello_required","duplicate_hello","unexpected_message"};
        const char *code="unknown";
        for(size_t i=0;i<sizeof codes/sizeof codes[0];i++)
            if(string_equals(j,"code",codes[i])){code=codes[i];break;}
        pm_logf("net profile=%s server_error=%s\n",profile_name(),code);
        retry();return;
    }
    if(kind!=12 && kind!=13)return;
    pm_token t;if(!token_from(j,&t))return;
    if(t.incarnation!=incarnation || t.session_epoch!=session_epoch ||
       t.screen_owner_epoch!=owner_epoch || t.display_epoch<display_epoch)return;
    // A validated newer FrameStart is also proof that the server accepted
    // the selection, even when its control reply is still in flight.
    if(await_selection && (kind!=12 || t.display_epoch<=display_epoch || pending_standby))return;
    if(kind==12){
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
        if(drawing)return;
#endif
        uint64_t w,h,len,crc;
        if(!number(j,"width",&w)||!number(j,"height",&h)||
           !number(j,"byte_len",&len)||!number(j,"crc32",&crc)||
           !string_equals(j,"format",PM_PIXEL_FORMAT_NAME) ||
           !pm_start(&assembly,t,w,h,PM_PIXEL_FORMAT_NAME,len,crc))pm_reset(&assembly);
        else {
#if PM_DISPLAY_TYPE == PM_DISPLAY_LCD
            receiving_stream=false;
            if(strstr(j,"\"stream\"")){
                uint64_t fps,sequence,generation;
                if(!number(j,"fps",&fps) || fps<1 || fps>30 || !number(j,"sequence",&sequence)
                    || !number(j,"generation",&generation)){pm_reset(&assembly);return;}
                receiving_stream=true;receiving_sequence=sequence;
                pm_video_prepare(&video,t,(uint32_t)fps,generation,video_now_us());
            }else pm_video_reset(&video);
#endif
            display_epoch=t.display_epoch;await_selection=false;
            // Web changes can leave SUB without a physical button response.
            // Only a validated current frame may replace the cached mode.
            if(string_equals(j,"mode","normal"))sub_mode=false;
            else if(string_equals(j,"mode","sub"))sub_mode=true;
            pm_trace_event(PM_TRACE_FRAME_START,t.frame_id,0,0);
        }
        return;
    }
    uint64_t offset;size_t count;
    if(number(j,"offset",&offset) && parse_bytes(j,&count))receive_chunk(t,offset,fragment,count);
}
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
static void poll_display(void){
    pm_display_poll();
    bool drawn;
    if(pm_display_take_result(&drawn)){
        drawing=false;
        if(drawn && state==ACTIVE && !await_selection && !panel_standby &&
           pm_token_current(drawing_token,incarnation,session_epoch,display_epoch,owner_epoch)){
            have_display=true;last_rendered=drawing_token;
            pending_render_token=drawing_token;pending_render_ack=true;
        }
    }
    if(panel_standby && !pm_display_busy()){panel_standby=false;standby();}
}
#endif
static void button(unsigned i,bool long_press){
#if PM_DIAGNOSTIC_STAGE
    pm_logf("diag button=%u state=%u\n",i,(unsigned)state);
#endif
    if(state==STANDBY){if(i==2 && (PM_BUTTON_COUNT==4 || long_press))start_wifi();return;}
    if(state==MENU){
        if(i==1||i==3){
            if(profile_ready(menu_profile^1)){
                menu_profile^=1;pm_display_status("NETWORK",menu_profile?"AWAY":"HOME");
            }else pm_display_status("AWAY","UNSET");
        }
        else if(i==0){
            active_profile=menu_profile;settings.profile=active_profile;store_settings();
            pm_logf("net profile=%s selected\n",profile_name());start_wifi();
        }
        else if(i==2)start_wifi();
        return;
    }
    if(state==WIFI||state==SERVER||state==SESSION||state==RETRY){
        if(i==0)menu();else if(i==2)standby();
        return;
    }
    if(state!=ACTIVE)return;
    if(i==2){
        if(!send_simple(7,"power")){retry();return;}
        if(sub_mode){pending_sub_cancel=true;await_selection=true;pm_reset(&assembly);}
        else {pending_standby=true;await_selection=true;started_ms=now_ms();pm_reset(&assembly);}
    }else if(i==0)send_simple(14,"enter_sub");
    else if(i==1){if(!sub_mode){await_selection=true;pm_reset(&assembly);}send_simple(15,"up");}
    else if(i==3){if(!sub_mode){await_selection=true;pm_reset(&assembly);}send_simple(16,"down");}
}
#if PM_DIAGNOSTIC_STAGE
static void diagnostic_hold(uint32_t duration_ms,bool watch_buttons){
    static const char *names[PM_BUTTON_COUNT]={"SUB","UP","POWER","DOWN"};
    uint32_t start=now_ms();
    while((uint32_t)(now_ms()-start)<duration_ms){
        cyw43_arch_poll();
        if(watch_buttons)for(unsigned i=0;i<input_count();i++)
            if(pm_button_step(&buttons[i],!gpio_get(button_pins[i]),now_ms())){
                pm_logf("diag local_button=%u\n",i);
                pm_display_status("BUTTON",names[i]);
            }
        sleep_ms(5);
    }
}
#endif
int main(void){
    stdio_init_all();
    bool net_ok=pm_net_init();
    if(net_ok){cyw43_arch_enable_sta_mode();wifi_enabled=true;}
    // Match the working YouTube order: CYW43 power/PIO first, panel reset next.
    pm_display_init();
    load_settings();active_profile=settings.profile;
    retry_ms=settings.retry_s*1000u;
    char board_id[2*PICO_UNIQUE_BOARD_ID_SIZE_BYTES+1];
    pico_get_unique_board_id_string(board_id,sizeof board_id);
    bool board_matches=strlen(PM_EXPECTED_BOARD_ID)==strlen(board_id);
    for(size_t i=0;board_matches && i<strlen(board_id);i++){
        char a=board_id[i],b=PM_EXPECTED_BOARD_ID[i];
        if(a>='a'&&a<='f')a-=32;
        if(b>='a'&&b<='f')b-=32;
        if(a!=b)board_matches=false;
    }
    if(!board_matches){pm_display_status("CONFIG","WRONG BOARD");while(true)sleep_ms(1000);}
    if(strcmp(PM_DEVICE_ID,PM_BUILD_DEVICE)){
        pm_display_status("CONFIG","WRONG ID");while(true)sleep_ms(1000);
    }
    if(strcmp(PM_DISPLAY_CAPABILITY,pm_display_capability())){
        pm_display_status("CONFIG","BAD DISPLAY");while(true)sleep_ms(1000);
    }
    size_t id_length=strlen(PM_DEVICE_ID);
    bool id_valid=id_length>0 && id_length<=50;
    for(size_t i=0;i<id_length;i++) {
        char c=PM_DEVICE_ID[i];
        if(!((c>='a'&&c<='z')||(c>='A'&&c<='Z')||
             (c>='0'&&c<='9')||c=='-'||c=='_'))id_valid=false;
    }
    if(!id_valid){pm_display_status("CONFIG","BAD ID");while(true)sleep_ms(1000);}
    for(unsigned i=0;i<input_count();i++){
        gpio_init(button_pins[i]);gpio_set_dir(button_pins[i],GPIO_IN);gpio_pull_up(button_pins[i]);
    }
    if(!net_ok){pm_display_status("WIFI","INIT ERROR");while(true)sleep_ms(1000);}
#if PM_DIAGNOSTIC_STAGE
    pm_display_reference_pattern();
    pm_logf("%s\n","diag stage=boot_pattern duration_ms=6000");
    diagnostic_hold(6000,false);
    pm_display_status("BUTTON","PRESS BUTTON");
    pm_logf("%s\n","diag stage=local_buttons duration_ms=25000");
    diagnostic_hold(25000,true);
    pm_logf("%s\n","diag stage=wifi_connect");
#endif
    start_wifi();
    while(true){
        uint64_t loop_started=pm_trace_clock();
        uint32_t now=now_ms();
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
        bool draining=panel_standby;
        poll_display();
        if(draining){pm_trace_flush();sleep_ms(5);continue;}
#endif
        for(unsigned i=0;i<input_count();i++){
            bool pressed=!gpio_get(button_pins[i]);
            bool was_pressed=buttons[i].stable;
            int event=pm_button_input_step(&buttons[i],PM_BUTTON_COUNT,i,pressed,now,settings.long_ms,state==STANDBY);
            if(!was_pressed && buttons[i].stable)pm_trace_event(PM_TRACE_BUTTON,0,(int32_t)i,(uint32_t)state);
            if(event>=0)button((unsigned)event,PM_BUTTON_COUNT==2 && (event==0 || event==2));
        }
        log_usb_snapshot();
        if(state==STANDBY){pm_trace_flush();sleep_ms(5);continue;}
        uint64_t poll_started=pm_trace_clock();
        cyw43_arch_poll();
        pm_trace_slow(PM_TRACE_SLOW_POLL,poll_started,1);
        // Button handling may save settings/draw and start a newer attempt.
        // Refresh before subtracting started_ms; the pre-button clock can wrap.
        now=now_ms();
        if(state==WIFI){
            log_wifi_progress();
            if(cyw43_tcpip_link_status(&cyw43_state,CYW43_ITF_STA)==CYW43_LINK_UP){
#if PM_DIAGNOSTIC_STAGE
                pm_display_reference_pattern();
                pm_logf("%s\n","diag stage=wifi_up_pattern duration_ms=6000");
                diagnostic_hold(6000,true);
#endif
                start_server();
            }else if(now-started_ms>settings.wifi_s*1000u)retry();
        }else if(state==SERVER||state==SESSION||state==ACTIVE){
            if(cyw43_tcpip_link_status(&cyw43_state,CYW43_ITF_STA)!=CYW43_LINK_UP){
                start_wifi();continue;
            }
            pm_net_poll();
            if(pm_net_state()==PM_NET_FAILED){retry();continue;}
            if(state==SERVER && pm_net_state()==PM_NET_READY){
                if(!send_hello()){retry();continue;}state=SESSION;
            }
            if(state!=ACTIVE && now-started_ms>settings.app_s*1000u){retry();continue;}
            uint8_t kind;
            // Bound cooperative receive work so image decoding cannot starve
            // network polling or physical input under continuous playback.
            unsigned budget=video_mode()?8u:2u;
            for(unsigned messages=0;messages<budget && pm_net_take(&kind,json,sizeof json);messages++){
                if(kind==18)process_frame_bytes((const uint8_t *)json,pm_net_payload_length());
                else process(kind,json);
                if(state==ACTIVE)pm_net_poll();
            }
            // Welcome/settings handling may also advance timers and draw.
            now=now_ms();
            if(!send_pending_render_ack()){retry();continue;}
            poll_video();
            if(state==ACTIVE && pending_standby && now-started_ms>2000)standby();
            if(state==ACTIVE && now-last_heartbeat_ms>=settings.heartbeat_s*1000u){
                heartbeat();last_heartbeat_ms=now;
            }
        }else if(state==RETRY){
            if(wifi_enabled && cyw43_tcpip_link_status(&cyw43_state,CYW43_ITF_STA)!=CYW43_LINK_UP){
                wifi_off();local("WIFI","RETRY");
            }
            if(now-started_ms>=retry_ms){
                retry_ms*=2;if(retry_ms>settings.retry_max_s*1000u)retry_ms=settings.retry_max_s*1000u;
                if(wifi_enabled)start_server();else start_wifi();
            }
        }
        pm_trace_slow(PM_TRACE_SLOW_LOOP,loop_started,(uint32_t)state);
        pm_trace_flush();sleep_ms(state==ACTIVE && video_mode()?1u:5u);
    }
}
