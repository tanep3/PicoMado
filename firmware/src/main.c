#include "trace.h"
#include "ack_probe.h"
#include "usb_control.h"
#include "device_api.h"
#include "input_rules.h"
#include "switch_feedback.h"
#include "observation.h"
#if PM_EVENT_STREAM
#include "observation_json.h"
#endif
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "pico/stdlib.h"
#include "pico/rand.h"
#include "pico/cyw43_arch.h"
#include "radio_query_sdk.h"
#include "pico/stdio_usb.h"
#include "hardware/flash.h"
#include "hardware/sync.h"
#include "hardware/regs/addressmap.h"
#include "core.h"
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
#include "eink_pending.h"
#endif
#include "display.h"
#include "net.h"
#include "video.h"
#include "stream_diag.h"
#include "endpoint_client.h"
#ifndef PM_AWAY_EEX1
#define PM_AWAY_EEX1 0
#endif
#ifndef PM_EEX1_PORT
#define PM_EEX1_PORT 48116
#endif

#if defined(PM_NORMAL_BUILD) && PM_NORMAL_BUILD
// Release selection is a build property. Keep the private device config intact.
#undef PM_DIAGNOSTIC_BOOT
#undef PM_DIAGNOSTIC_STAGE
#define PM_DIAGNOSTIC_BOOT 0
#define PM_DIAGNOSTIC_STAGE 0
#endif

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
static enum { WIFI_PM_UNREQUESTED, WIFI_PM_PENDING, WIFI_PM_CHECKED } wifi_pm_status;
static int wifi_pm_request_result,wifi_pm_readback_result;
static uint32_t wifi_pm_readback;
static pm_radio_comparison radio_comparison;
static pm_roaming_comparison roaming_comparison;
static bool radio_defaults_attempted;
static pm_radio_write_readback default_interference,default_roaming;
static pm_token last_rendered;
static pm_token pending_render_token;
static bool pending_render_ack;
static pm_switch_feedback switching;
#if PM_DISPLAY_TYPE == PM_DISPLAY_LCD
static bool lcd_waiting_frame;
static pm_token lcd_waiting_token;
#endif
// Physical pixels may outlive their invalidated token during normal recovery.
static bool retained_pixels;
#if PM_EVENT_STREAM
static uint64_t input_operation;
#endif
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
static pm_eink_pending pending_frame;
#endif

static pm_assembly assembly;
static void reset_assembly(void){
#if PM_DISPLAY_TYPE == PM_DISPLAY_LCD
    if(lcd_waiting_frame)pm_switch_forget(&switching,lcd_waiting_token);
    lcd_waiting_frame=false;
#endif
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
    if(pending_frame.phase==PM_EINK_READY)pm_switch_forget(&switching,pending_frame.frame.token);
    if(pending_frame.phase!=PM_EINK_EMPTY)
        pm_observe(PM_OB_FRAME,1,PM_OB_CANCELLED,0,0,pending_frame.frame.token.session_epoch,pending_frame.frame.token.frame_id,0,0);
    else
#endif
    if(assembly.active)pm_observe(PM_OB_FRAME,1,PM_OB_CANCELLED,0,0,assembly.token.session_epoch,assembly.token.frame_id,0,0);
    pm_diag_abort();pm_net_rx_frame_end(PM_RX_RESET,0);pm_reset(&assembly);
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
    pm_eink_pending_reset(&pending_frame);
#endif
}
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
#if PM_AWAY_EEX1
_Static_assert(sizeof(PM_DEVICE_ID)<=33,"EEX1 client ID must fit32 ASCII characters");
static pm_endpoint_client endpoint_client;
static pm_profile endpoint_profile;
static char endpoint_ip[16];
static bool endpoint_initialized;
static uint64_t endpoint_now(void){return time_us_64()/1000;}
static void endpoint_reset(void){if(endpoint_initialized)pm_endpoint_client_reset(&endpoint_client);}
static void endpoint_result(pm_eex_status status,pm_eex_stage stage){
    if(active_profile && endpoint_initialized)
        pm_endpoint_client_result(&endpoint_client,status,stage,endpoint_now());
}
static void endpoint_snapshot(void){
    if(!endpoint_initialized || !active_profile)return;
    pm_endpoint_client *c=&endpoint_client;
    uint32_t ip=c->network.local_ipv4,gw=c->network.gateway_ipv4,mask=c->observed.netmask;
    uint32_t proxy=c->cache.valid?c->cache.endpoint.ipv4:
        (c->exchange.offer.source_host?c->exchange.reporter.ipv4:c->exchange.offer.host);
    pm_logf("net endpoint source=EEX1 wifi_gen=%llu phase=%u verified=%u udp_open=%u receipt=%u result=%u\n",
        (unsigned long long)c->network.wifi_generation,(unsigned)c->exchange.phase,c->cache.valid?1u:0u,
        c->opened?1u:0u,c->exchange.receipt_confirmed?1u:0u,c->exchange.result_confirmed?1u:0u);
    pm_logf("net endpoint local=%u.%u.%u.%u gateway=%u.%u.%u.%u netmask=%u.%u.%u.%u\n",
        (unsigned)(ip>>24),(unsigned)((ip>>16)&255),(unsigned)((ip>>8)&255),(unsigned)(ip&255),
        (unsigned)(gw>>24),(unsigned)((gw>>16)&255),(unsigned)((gw>>8)&255),(unsigned)(gw&255),
        (unsigned)(mask>>24),(unsigned)((mask>>16)&255),(unsigned)((mask>>8)&255),(unsigned)(mask&255));
    pm_logf("net endpoint candidate=%u.%u.%u.%u socks_port=%u report_udp_port=%u tx=%lu rx=%lu tx_error=%lu\n",
        (unsigned)(proxy>>24),(unsigned)((proxy>>16)&255),(unsigned)((proxy>>8)&255),(unsigned)(proxy&255),
        (unsigned)(c->cache.valid?c->cache.endpoint.port:c->exchange.offer.port),(unsigned)c->exchange.reporter.port,
        (unsigned long)c->sent,(unsigned long)c->received,(unsigned long)c->send_errors);
}
#endif

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
    case STANDBY:return "STANDBY";case RETRY:return "RETRY";case ENDPOINT:return "ENDPOINT";default:return "UNKNOWN";}
}
static const char *transport_name(void){
    switch(pm_net_state()){case PM_NET_DOWN:return "DOWN";case PM_NET_TCP:return "TCP";
    case PM_NET_SOCKS:return "SOCKS";case PM_NET_WS:return "WS";case PM_NET_READY:return "READY";
    case PM_NET_DRAINING:return "DRAINING";case PM_NET_DRAINED:return "DRAINED";
    case PM_NET_FAILED:return "FAILED";default:return "UNKNOWN";}
}
static void log_wifi_pm_status(void){
    if(wifi_pm_status==WIFI_PM_UNREQUESTED)return;
    if(wifi_pm_status==WIFI_PM_PENDING){
        pm_logf("net wifi_pm request_sdk_result=%d readback=pending\n",wifi_pm_request_result);
    }else if(wifi_pm_readback_result){
        pm_logf("net wifi_pm request_sdk_result=%d readback_sdk_result=%d\n",
            wifi_pm_request_result,wifi_pm_readback_result);
    }else{
        unsigned mode=wifi_pm_readback&15u;
        pm_logf("net wifi_pm request_sdk_result=%d readback_sdk_result=0 raw=0x%08lx mode=%u none=%u\n",
            wifi_pm_request_result,(unsigned long)wifi_pm_readback,mode,
            mode==CYW43_NO_POWERSAVE_MODE?1u:0u);
    }
}
static void check_wifi_pm(void){
    if(wifi_pm_status!=WIFI_PM_PENDING)return;
    // Once per successful Wi-Fi join, outside the video/heartbeat hot path.
    cyw43_arch_lwip_begin();
    wifi_pm_readback_result=cyw43_wifi_get_pm(&cyw43_state,&wifi_pm_readback);
    cyw43_arch_lwip_end();
    wifi_pm_status=WIFI_PM_CHECKED;
    log_wifi_pm_status();
}
/* Replay startup attempt facts, not a fresh query of current RF state. */
static void log_radio_defaults_status(void){
 if(!radio_defaults_attempted)return;
 pm_interference_candidate interference;pm_roaming_candidate roaming;uint32_t got;
 bool a=pm_radio_transport_candidate(default_interference.read,&interference)&&
        pm_interference_candidate_value(interference,&got)&&got==3;
 bool b=pm_roaming_transport(default_roaming.read,&roaming)&&pm_roaming_value(roaming,&got)&&got==1;
 pm_logf("radio defaults k=interference t=3 w=%d r=%d s=%s x=%08lx m=%u cdc_status=unverified\n",
   default_interference.write_result,default_interference.read.sdk_result,pm_radio_status_name(default_interference.read.status),
   (unsigned long)default_interference.read.raw,a?1u:0u);
 pm_logf("radio defaults k=roam_off t=1 w=%d r=%d s=%s x=%08lx m=%u cdc_status=unverified\n",
   default_roaming.write_result,default_roaming.read.sdk_result,pm_radio_status_name(default_roaming.read.status),
   (unsigned long)default_roaming.read.raw,b?1u:0u);
}
static void apply_radio_defaults(void){
 if(radio_defaults_attempted||!wifi_enabled||state!=WIFI||!pm_radio_thread_context()||
    cyw43_tcpip_link_status(&cyw43_state,CYW43_ITF_STA)!=CYW43_LINK_UP)return;
 /* Once per enabled STA interval, including errors. No loop or baseline/ID
  * mutation. Association is complete; do not apply before firmware bringup. */
 radio_defaults_attempted=true;
 pm_interference_candidate interference={0};pm_roaming_candidate roaming={0};
 (void)pm_interference_candidate_from_raw(3,&interference);(void)pm_roaming_from_raw(1,&roaming);
 default_interference.write_result=pm_radio_interference_write(interference);
 default_interference.read=pm_radio_interference_get(true);
 default_roaming.write_result=pm_radio_roaming_write(roaming);
 default_roaming.read=pm_radio_roaming_get(true);
 log_radio_defaults_status();
}
static bool radio_allowed_for(pm_radio_read *rejection,bool recovery){
    if(!pm_radio_thread_context()){*rejection=pm_radio_rejected(PM_RADIO_CONTEXT);return false;}
    bool available=wifi_enabled&&(recovery?(state!=MENU&&state!=STANDBY):
        (state==ACTIVE&&have_display&&cyw43_tcpip_link_status(&cyw43_state,CYW43_ITF_STA)==CYW43_LINK_UP));
    if(!available){*rejection=pm_radio_rejected(PM_RADIO_UNAVAILABLE);return false;}
    bool busy=switching.waiting||switching.sent!=switching.replied||await_selection||
        pending_standby||pending_sub_cancel||pending_render_ack||assembly.active;
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
    busy=busy||panel_standby||drawing||pending_frame.phase!=PM_EINK_EMPTY||pm_display_busy();
#else
    busy=busy||lcd_waiting_frame||receiving_stream||video.active||video.ready||video.busy;
#endif
    if(busy){*rejection=pm_radio_rejected(PM_RADIO_BUSY);return false;}
    return true;
}
static bool radio_allowed(pm_radio_read *rejection){return radio_allowed_for(rejection,false);}
static pm_radio_read usb_roaming_query(void){
 pm_radio_read rejection;return radio_allowed(&rejection)?pm_radio_roaming_get(true):rejection;
}
static pm_roaming_change usb_roaming_change(pm_roaming_op op){
 pm_radio_read rejection;return radio_allowed_for(&rejection,op==PM_ROAM_RESTORE)?
  pm_radio_roaming_change(&roaming_comparison,op):pm_roaming_rejected(op,rejection);
}
static pm_radio_read usb_radio_query(void){
    pm_radio_read rejection;
    return radio_allowed(&rejection)?pm_radio_interference_get(true):rejection;
}
static pm_radio_change usb_radio_change(pm_radio_change_op op){
    pm_radio_read rejection;
    return radio_allowed(&rejection)?pm_radio_interference_change(&radio_comparison,op):
        pm_radio_change_rejected(op,rejection);
}
static void log_usb_snapshot(void){
    static bool was_connected;
    static uint32_t last_ms;
    bool connected=stdio_usb_connected();
#if PM_ACK_PATH_PROBE
    pm_ack_probe_live(session_epoch,state==ACTIVE);
#endif
    if(connected)for(unsigned n=0;n<16;n++){int ch=getchar_timeout_us(0);if(ch<0)break;pm_usb_control_input(ch);}
    pm_usb_control_poll(connected);
    pm_usb_control_service_radio(connected,usb_radio_query);
    pm_usb_control_service_radio_change(connected,usb_radio_change);
    pm_usb_control_service_roaming(connected,usb_roaming_query,usb_roaming_change);
    if(connected && !was_connected)pm_net_rx_snapshot();
    pm_stats_usb_poll(connected);
    if(!connected){was_connected=false;return;}
    uint32_t now=now_ms();
    if(was_connected && now-last_ms<15000)return;
    was_connected=true;last_ms=now;
    int wifi=wifi_enabled?cyw43_wifi_link_status(&cyw43_state,CYW43_ITF_STA):CYW43_LINK_DOWN;
    int tcpip=wifi_enabled?cyw43_tcpip_link_status(&cyw43_state,CYW43_ITF_STA):CYW43_LINK_DOWN;
    pm_logf("net status profile=%s app=%s transport=%s wifi=%s tcpip=%s ip_assigned=%u\n",
        profile_name(),app_name(),transport_name(),link_name(wifi),link_name(tcpip),
        tcpip==CYW43_LINK_UP?1u:0u);
#if PM_AWAY_EEX1
    endpoint_snapshot();
#endif
    // Replay cached results for late USB attachment; never repeat radio IOCTLs.
    log_wifi_pm_status();
    log_radio_defaults_status();
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
        (!p->socks || (((PM_AWAY_EEX1 && index==1) || (p->proxy_ip && p->proxy_ip[0] && p->proxy_port)) &&
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
static void invalidate_display_context(void){
    pm_switch_reset(&switching);pm_display_feedback(PM_DISPLAY_FEEDBACK_REPLACE);
    if(pending_render_ack)pm_ack_probe_note(PM_AP_PENDING_CLEAR,9,pending_render_token.session_epoch,pending_render_token.frame_id,(uint32_t)state,0,0,0);
    owner_epoch++;reset_assembly();have_display=false;pending_render_ack=false;
}
static void local(const char *one,const char *two) {
    retained_pixels=false;invalidate_display_context();
    pm_display_wake();pm_display_status(one,two);
#if PM_DIAGNOSTIC_BOOT || PM_DIAGNOSTIC_STAGE
    pm_logf("lcd local: %s / %s\n",one,two);
#endif
}
static void connection_status(const char *one,const char *two){
    if(retained_pixels)invalidate_display_context();else local(one,two);
}
static void wifi_off(void){
#if PM_AWAY_EEX1
    endpoint_reset();
#endif
    pm_net_close();
    if(wifi_enabled){radio_defaults_attempted=false;pm_radio_comparison_invalidate(&radio_comparison);pm_roaming_invalidate(&roaming_comparison);cyw43_arch_disable_sta_mode();wifi_enabled=false;}
}
static void standby(void){
    retained_pixels=false;
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
    pm_display_standby();
    if(pm_display_busy()){
        if(!panel_standby)owner_epoch++;
        if(pending_render_ack)pm_ack_probe_note(PM_AP_PENDING_CLEAR,9,pending_render_token.session_epoch,pending_render_token.frame_id,(uint32_t)state,0,0,0);
    panel_standby=true;reset_assembly();pending_render_ack=false;
        pending_standby=false;wifi_off();return;
    }
#endif
    pm_display_standby();
#if PM_DIAGNOSTIC_BOOT
    pm_logf("%s\n","lcd black: standby");
#endif
    if(pending_render_ack)pm_ack_probe_note(PM_AP_PENDING_CLEAR,9,pending_render_token.session_epoch,pending_render_token.frame_id,(uint32_t)state,0,0,0);
    owner_epoch++;reset_assembly();have_display=false;pending_render_ack=false;
    wifi_off();pending_standby=false;state=STANDBY;
}
static void start_wifi(void){
#if PM_AWAY_EEX1
    endpoint_reset();
#endif
    pm_net_close();
    if(!wifi_enabled){
        cyw43_arch_enable_sta_mode();wifi_enabled=true;
    }
    // main enables STA before panel init, so apply this also on first boot.
    // Streaming/control use an awake radio; standby still disables STA.
    cyw43_arch_lwip_begin();
    wifi_pm_request_result=cyw43_wifi_pm(&cyw43_state,CYW43_NONE_PM);
    cyw43_arch_lwip_end();
    wifi_pm_status=WIFI_PM_PENDING;
    pm_logf("net wifi_pm requested=none sdk_result=%d\n",wifi_pm_request_result);
    incarnation=0;session_epoch=0;display_epoch=0;
    await_selection=false;sub_mode=false;pending_standby=false;pending_sub_cancel=false;
    connection_status("WIFI", "CONNECTING");
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
static void start_server_now(const pm_profile *connection){
    incarnation=0;session_epoch=0;display_epoch=0;
    await_selection=false;sub_mode=false;pending_standby=false;pending_sub_cancel=false;
    connection_status("SERVER","CONNECTING");pm_net_begin(connection);
    state=SERVER;started_ms=now_ms();
}
#if PM_AWAY_EEX1
static void endpoint_connect(pm_endpoint e){
    endpoint_profile=profiles[active_profile];
    snprintf(endpoint_ip,sizeof endpoint_ip,"%u.%u.%u.%u",(unsigned)(e.ipv4>>24),
        (unsigned)((e.ipv4>>16)&255),(unsigned)((e.ipv4>>8)&255),(unsigned)(e.ipv4&255));
    endpoint_profile.proxy_ip=endpoint_ip;endpoint_profile.proxy_port=e.port;
    start_server_now(&endpoint_profile);
}
#endif
static void start_server(void){
#if PM_AWAY_EEX1
    if(active_profile){
        pm_endpoint e;
        (void)pm_endpoint_client_begin(&endpoint_client,PM_DEVICE_ID,endpoint_now());
        if(pm_endpoint_client_take(&endpoint_client,endpoint_now(),&e)){endpoint_connect(e);return;}
        connection_status("NETWORK","AWAY");state=ENDPOINT;started_ms=now_ms();return;
    }
#endif
    start_server_now(&profiles[active_profile]);
}
static void retry_with_image(bool keep_image){
#if PM_AWAY_EEX1
    endpoint_result(PM_EEX_FAIL,PM_EEX_PROTOCOL_ERROR);
#endif
    bool wifi_was_up=wifi_enabled &&
        cyw43_tcpip_link_status(&cyw43_state,CYW43_ITF_STA)==CYW43_LINK_UP;
    log_wifi_progress();
    pm_logf("net profile=%s retry app=%s transport=%s delay_ms=%lu\n",
        profile_name(),app_name(),transport_name(),(unsigned long)retry_ms);
    // A failed server/proxy session does not invalidate a healthy Wi-Fi/IP link.
    if(wifi_was_up)pm_net_close();else wifi_off();
    if(keep_image){
        pm_display_feedback(PM_DISPLAY_FEEDBACK_RESTORE);
        retained_pixels=true;
        // Invalidate ownership immediately but keep the last valid pixels on
        // glass during backoff. No old token may be drawn after reconnect.
        invalidate_display_context();
    }else local(wifi_was_up?"SERVER":"WIFI",wifi_was_up?"CONNECTING":"RETRY");
    state=RETRY;started_ms=now_ms();
}
static void retry(void){retry_with_image(false);}
static void retry_recovery(void){retry_with_image(retained_pixels);}
#if PM_DISPLAY_TYPE == PM_DISPLAY_LCD
static void retry_eof(void){
    bool keep=retained_pixels || (have_display && !await_selection && !pending_standby &&
        pm_token_current(last_rendered,incarnation,session_epoch,display_epoch,owner_epoch));
    retry_with_image(keep);
}
#endif
// Every transport failure checkpoint uses one recovery policy. In particular,
// an EOF deadline found inside receive/process/draw must retain the same pixels
// as one found by the outer network poll. This check does not poll the SDK.
static bool recover_transport_failure(void){
    if(pm_net_checkpoint()!=PM_NET_FAILED)return false;
#if PM_DISPLAY_TYPE == PM_DISPLAY_LCD
    pm_net_failure failure=pm_net_last_failure();
    if(failure==PM_NET_FAIL_EOF_TIMEOUT)retry_eof();
    else if(state==SERVER && failure==PM_NET_FAIL_TCP)retry_recovery();
    else retry();
#else
    retry();
#endif
    return true;
}
#if PM_DISPLAY_TYPE == PM_DISPLAY_LCD
// A network reset can precede the usual poll checkpoint (Wi-Fi loss or EEX
// network change). Preserve normal EOF pixels, but let hard errors win first.
static bool recover_transport_loss(void){
    if(recover_transport_failure())return true;
    pm_net_stage transport=pm_net_state();
    if(transport!=PM_NET_DRAINING && transport!=PM_NET_DRAINED)return false;
    retry_eof();return true;
}
#endif
#if PM_AWAY_EEX1
static void endpoint_proxy_progress(void){
    if(!active_profile)return;
    if(pm_net_proxy_target_connected())endpoint_result(PM_EEX_OK,PM_EEX_TARGET_CONNECTED);
    else if(pm_net_state()==PM_NET_FAILED){
        pm_net_failure f=pm_net_last_failure();
        endpoint_result(PM_EEX_FAIL,f==PM_NET_FAIL_TCP?PM_EEX_TCP_FAILED:
            f==PM_NET_FAIL_TARGET?PM_EEX_TARGET_REJECTED:PM_EEX_PROTOCOL_ERROR);
    }
}
static void poll_endpoint(void){
    if(!endpoint_initialized)return;
    pm_endpoint_client_poll(&endpoint_client,endpoint_now());
    bool changed=pm_endpoint_client_network_changed(&endpoint_client);
    if(changed && endpoint_client.network.ipv4_ready && state!=WIFI && state!=MENU && state!=STANDBY){
#if PM_DISPLAY_TYPE == PM_DISPLAY_LCD
        if((state==SERVER || state==SESSION || state==ACTIVE) && recover_transport_loss())return;
#endif
        retry_recovery();return;
    }
    if(state==ENDPOINT){
        if(cyw43_tcpip_link_status(&cyw43_state,CYW43_ITF_STA)!=CYW43_LINK_UP){start_wifi();return;}
        pm_endpoint e;
        if(pm_endpoint_client_take(&endpoint_client,endpoint_now(),&e))endpoint_connect(e);
        else if(endpoint_client.failed)retry_recovery();
    }
}
#endif
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
#if PM_EVENT_STREAM
    pm_observation_drain(6,768,200);
#endif
    pm_ack_probe_context(session_epoch,0);
    uint64_t parent=0;
#if PM_EVENT_STREAM
    parent=input_operation;
#endif
    uint64_t op=pm_observe(PM_OB_NETWORK,kind,PM_OB_DISPATCHED,0,parent,session_epoch,0,0,0);
    int n;
#if PM_EVENT_STREAM
    pm_observation_status observed=pm_observation_read();
    if(parent && observed.enabled)n=snprintf(outbound_json,sizeof outbound_json,
        "{\"type\":\"%s\",\"data\":{\"incarnation\":%llu,\"session_epoch\":%llu},"
        "\"observation\":{\"version\":1,\"boot\":%llu,\"operation\":%llu}}",
        type,(unsigned long long)incarnation,(unsigned long long)session_epoch,
        (unsigned long long)observed.boot,(unsigned long long)parent);
    else
#endif
    n=snprintf(outbound_json,sizeof outbound_json,
        "{\"type\":\"%s\",\"data\":{\"incarnation\":%llu,\"session_epoch\":%llu}}",
        type,(unsigned long long)incarnation,(unsigned long long)session_epoch);
    bool ok=n>0 && (size_t)n<sizeof outbound_json &&
#if PM_EVENT_STREAM
        pm_net_send_observed(kind,outbound_json,op,parent,session_epoch);
#else
        pm_net_send(kind,outbound_json);
#endif
    pm_observe(PM_OB_NETWORK,kind,ok?PM_OB_PENDING:PM_OB_ERROR,op,parent,session_epoch,0,0,0);
#if PM_EVENT_STREAM
    pm_observation_drain(6,768,200);
#endif
    return ok;
}
static bool send_button_input(uint8_t kind,const char *type,uint64_t accepted_us){
    bool ok=send_simple(kind,type);
    if(ok){
        pm_token scope={incarnation,session_epoch,display_epoch,owner_epoch,
                        have_display?last_rendered.frame_id:0};
        pm_switch_sent(&switching,scope,accepted_us,kind!=7);
    }else pm_switch_cancel(&switching);
    if(kind==7 || !ok)pm_display_feedback(PM_DISPLAY_FEEDBACK_REPLACE);
    return ok;
}
/* Keep cancellation in the model and adapter on one path. The adapter alone
 * knows whether identical text is already visible or preparation is pending. */
static void prioritize_received_image(void){
    if(pm_switch_cancel_hint_for_image(&switching))
        pm_display_feedback(PM_DISPLAY_FEEDBACK_REPLACE);
}
static uint64_t observed_parent(const char *j){
#if PM_EVENT_STREAM
    uint64_t operation;
    pm_observation_status observed=pm_observation_read();
    if(observed.enabled && (strstr(j,"observation") || strstr(j,"\\u")) && pm_observation_json_cause(j,observed.boot,&operation))return operation;
#else
    (void)j;
#endif
    return 0;
}
static bool send_ack(uint8_t kind,const char *type,pm_token t){
    pm_ack_probe_context(t.session_epoch,t.frame_id);
    pm_ack_probe_note(PM_AP_ACK_BUILD,kind,t.session_epoch,t.frame_id,(uint32_t)t.display_epoch,(uint32_t)(t.display_epoch>>32),(uint32_t)t.screen_owner_epoch,(uint32_t)(t.screen_owner_epoch>>32));
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
    pm_trace_frame_context(0);pm_ack_probe_context(0,0);return ok;
}
static bool send_pending_render_ack(void){
    if(state!=ACTIVE || !pending_render_ack)return true;
    pm_token t=pending_render_token;pending_render_ack=false;
    pm_ack_probe_note(PM_AP_PENDING_TAKE,9,t.session_epoch,t.frame_id,0,0,0,0);
    if(!pm_token_current(t,incarnation,session_epoch,display_epoch,owner_epoch)){pm_ack_probe_note(PM_AP_PENDING_SKIP,9,t.session_epoch,t.frame_id,1,0,0,0);return true;}
    // The peer ended application input; do not send data after its WS Close.
    if(pm_net_state()==PM_NET_DRAINED){pm_ack_probe_note(PM_AP_PENDING_SKIP,9,t.session_epoch,t.frame_id,2,0,0,0);return true;}
    return send_ack(9,"rendered_ack",t);
}
#if PM_DISPLAY_TYPE == PM_DISPLAY_LCD
// Functional deadlines must advance even when diagnostic tracing is disabled.
static uint64_t video_now_us(void){return time_us_64();}
static bool video_mode(void){return video.active;}
static void poll_video(void){
    if(state!=ACTIVE || !video.active)return;
    if(await_selection || pending_standby){video.ready=false;return;}
    if(video.ready && !pm_switch_eligible(&switching,video.token))return;
    if(!pm_token_current(video.context,incarnation,session_epoch,display_epoch,owner_epoch)){
        pm_video_reset(&video);return;
    }
    uint64_t now=video_now_us();
    if(recover_transport_failure())return;
    uint8_t *pixels=pm_video_begin_draw(&video,now);
    if(pixels){
        pm_trace_frame_context(video.token.frame_id);
        pm_token drawing=video.token;
        uint64_t draw_op=pm_observe(PM_OB_DISPLAY,1,PM_OB_DISPATCHED,0,0,drawing.session_epoch,drawing.frame_id,0,1);
        bool ok=pm_display_frame(pixels,PM_IMAGE_BYTES);pm_trace_frame_context(0);
        pm_observe(PM_OB_DISPLAY,1,ok?PM_OB_COMPLETED:PM_OB_ERROR,draw_op,0,drawing.session_epoch,drawing.frame_id,1,1);
        if(!ok){pm_video_reset(&video);retry();return;}
        pm_video_finish_draw(&video,video_now_us());retained_pixels=false;have_display=true;last_rendered=video.last_rendered;
        pm_switch_complete(&switching,drawing);
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
        PM_DISPLAY_TYPE==PM_DISPLAY_LCD?",\"lcd-push-v1\"":",\"eink-clock-v1\"",(unsigned long long)owner_epoch);
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
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
static pm_api_result dispatch_pending_eink(void){
    if(pending_frame.phase!=PM_EINK_READY)return PM_API_PENDING;
    bool allowed=state==ACTIVE && !await_selection && !panel_standby;
    if(!allowed || !pm_token_current(pending_frame.frame.token,incarnation,session_epoch,display_epoch,owner_epoch)){
        reset_assembly();return PM_API_REJECTED;
    }
    if(!pm_switch_eligible(&switching,pending_frame.frame.token))return PM_API_PENDING;
    pm_eink_frame next;
    if(!pm_eink_pending_take(&pending_frame,!drawing && !pm_display_busy(),allowed,
        incarnation,session_epoch,display_epoch,owner_epoch,&next))return PM_API_PENDING;
    pm_token t=next.token;
    // A replay may become Ready while its original physical draw is in flight.
    // Completion changes last_rendered after receive_chunk's earlier check.
    if(have_display && pm_token_equal(last_rendered,t)){
        pending_render_token=t;pending_render_ack=true;
        pm_ack_probe_note(PM_AP_PENDING_SET,9,t.session_epoch,t.frame_id,0,0,0,0);
        return PM_API_COMPLETED;
    }
    bool same_display=have_display && last_rendered.incarnation==t.incarnation &&
        last_rendered.session_epoch==t.session_epoch && last_rendered.display_epoch==t.display_epoch &&
        last_rendered.screen_owner_epoch==t.screen_owner_epoch;
    pm_trace_frame_context(t.frame_id);
    pm_ack_probe_note(PM_AP_DRAW_BEGIN,0,t.session_epoch,t.frame_id,0,0,0,0);
    uint64_t op=pm_observe(PM_OB_DISPLAY,1,PM_OB_DISPATCHED,0,0,t.session_epoch,t.frame_id,0,0);
    bool accepted=next.refresh_minutes ? pm_display_clock_frame(assembly.pixels,PM_IMAGE_BYTES,
        next.refresh_minutes,next.refresh_minute,!same_display) : pm_display_frame(assembly.pixels,PM_IMAGE_BYTES);
    pm_trace_frame_context(0);
    pm_ack_probe_note(PM_AP_DRAW_END,0,t.session_epoch,t.frame_id,accepted,0,0,0);
    pm_observe(PM_OB_DISPLAY,1,accepted?PM_OB_PENDING:PM_OB_ERROR,op,0,t.session_epoch,t.frame_id,0,0);
    if(!accepted){retry();return PM_API_ERROR;}
    drawing=true;drawing_token=t;return PM_API_PENDING;
}
#endif
#if PM_DISPLAY_TYPE == PM_DISPLAY_LCD
static pm_api_result finish_lcd_image(pm_token t){
    if(!pm_token_current(t,incarnation,session_epoch,display_epoch,owner_epoch))return PM_API_REJECTED;
    if(!pm_switch_eligible(&switching,t)){
        lcd_waiting_token=t;lcd_waiting_frame=true;return PM_API_PENDING;
    }
    lcd_waiting_frame=false;
    if(!have_display || !pm_token_equal(last_rendered,t)) {
#if PM_DIAGNOSTIC_BOOT
        size_t nonzero=0;
        for(size_t i=0;i<PM_IMAGE_BYTES;i++)if(assembly.pixels[i])nonzero++;
        pm_logf("frame %llu: nonzero bytes %u/%u\n",
               (unsigned long long)t.frame_id,(unsigned)nonzero,(unsigned)PM_IMAGE_BYTES);
        pm_display_reference_pattern();sleep_ms(500);
#endif
#if PM_DISPLAY_TYPE == PM_DISPLAY_LCD
        if(recover_transport_failure())return PM_API_ERROR;
#endif
        pm_trace_frame_context(t.frame_id);
        pm_ack_probe_note(PM_AP_DRAW_BEGIN,0,t.session_epoch,t.frame_id,0,0,0,0);
        uint64_t draw_op=pm_observe(PM_OB_DISPLAY,1,PM_OB_DISPATCHED,0,0,t.session_epoch,t.frame_id,0,0);
        bool drawn=pm_display_frame(assembly.pixels,PM_IMAGE_BYTES);
        pm_trace_frame_context(0);
        pm_ack_probe_note(PM_AP_DRAW_END,0,t.session_epoch,t.frame_id,drawn,0,0,0);
        pm_observe(PM_OB_DISPLAY,1,drawn?PM_OB_COMPLETED:PM_OB_ERROR,draw_op,0,t.session_epoch,t.frame_id,1,0);
        if(!drawn){retry();return PM_API_ERROR;}
        retained_pixels=false;have_display=true;last_rendered=t;
#if PM_DIAGNOSTIC_STAGE
        pm_logf("diag frame_spi_complete=%llu\n",(unsigned long long)t.frame_id);
#endif
#if PM_DIAGNOSTIC_BOOT
        pm_logf("lcd SPI complete: frame %llu\n",(unsigned long long)t.frame_id);
#endif
    }
    pm_switch_complete(&switching,t);
    if(pending_render_ack)pm_ack_probe_note(PM_AP_PENDING_REPLACED,9,pending_render_token.session_epoch,pending_render_token.frame_id,(uint32_t)t.frame_id,(uint32_t)(t.frame_id>>32),0,0);
    pending_render_token=t;pending_render_ack=true;
    pm_ack_probe_note(PM_AP_PENDING_SET,9,t.session_epoch,t.frame_id,0,0,0,0);
    return PM_API_COMPLETED;
}
#endif
static pm_api_result receive_chunk(pm_token t,uint64_t offset,const uint8_t *bytes,size_t count){
    if(state!=ACTIVE || await_selection || !pm_token_current(t,incarnation,session_epoch,display_epoch,owner_epoch) ||
       !assembly.active || !pm_token_equal(assembly.token,t)){pm_diag_event(PM_D_CONTEXT,t.session_epoch,t.frame_id,0,0);pm_observe(PM_OB_FRAME,1,PM_OB_REJECTED,0,0,t.session_epoch,t.frame_id,2,0);return PM_API_REJECTED;}
    bool complete;bool valid_offset=offset<=UINT32_MAX;
    if(!valid_offset)pm_reset(&assembly);
    if(!valid_offset || !pm_chunk(&assembly,t,offset,bytes,count,&complete)){
#if PM_DISPLAY_TYPE == PM_DISPLAY_LCD
        if(!receiving_stream)
#endif
        {pm_ack_probe_note(PM_AP_FRAME_CHECK,0,t.session_epoch,t.frame_id,1,0,0,0);}
        pm_diag_event(PM_D_INVALID,t.session_epoch,t.frame_id,0,(uint32_t)offset);pm_net_rx_frame_end(PM_RX_INVALID,0);pm_observe(PM_OB_FRAME,1,PM_OB_REJECTED,0,0,t.session_epoch,t.frame_id,1,0);
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
        pm_eink_pending_reset(&pending_frame);
#endif
        return PM_API_REJECTED;}
    pm_rx_progress((uint32_t)assembly.used);
#if PM_DISPLAY_TYPE == PM_DISPLAY_LCD
    if(receiving_stream)pm_diag_progress((uint32_t)assembly.used);
#endif
    if(!complete)return PM_API_PENDING;
    pm_net_rx_frame_end(PM_RX_COMPLETE,0);
    pm_trace_event(PM_TRACE_CRC_OK,t.frame_id,0,PM_IMAGE_BYTES);
    pm_observe(PM_OB_FRAME,1,PM_OB_COMPLETED,0,0,t.session_epoch,t.frame_id,PM_IMAGE_BYTES,0);
    #if PM_DISPLAY_TYPE == PM_DISPLAY_LCD
    if(!receiving_stream)
#endif
    pm_ack_probe_note(PM_AP_FRAME_CHECK,0,t.session_epoch,t.frame_id,0,0,0,0);
    if(!pm_token_current(t,incarnation,session_epoch,display_epoch,owner_epoch))return PM_API_REJECTED;
    pm_switch_receive(&switching,t);
    prioritize_received_image();
#if PM_DISPLAY_TYPE == PM_DISPLAY_LCD
    if(receiving_stream){
        pm_diag_frame_done();
        return pm_video_offer(&video,t,receiving_sequence,assembly.pixels,PM_IMAGE_BYTES)?PM_API_PENDING:PM_API_REJECTED;
    }
#endif
    if(!send_ack(8,"receipt_ack",t)){if(!recover_transport_failure())retry();return PM_API_ERROR;}
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
    if(!have_display || !pm_token_equal(last_rendered,t)){
        if(!pm_eink_pending_complete(&pending_frame,t))return PM_API_REJECTED;
        return dispatch_pending_eink();
    }
    pm_eink_pending_reset(&pending_frame);
#else
    return finish_lcd_image(t);
#endif
    if(pending_render_ack)pm_ack_probe_note(PM_AP_PENDING_REPLACED,9,pending_render_token.session_epoch,pending_render_token.frame_id,(uint32_t)t.frame_id,(uint32_t)(t.frame_id>>32),0,0);
    pending_render_token=t;pending_render_ack=true;
    pm_ack_probe_note(PM_AP_PENDING_SET,9,t.session_epoch,t.frame_id,0,0,0,0);
    return PM_API_COMPLETED;
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
#if PM_EVENT_STREAM
    pm_net_observed_api(0,t.frame_id);
#endif
    receive_chunk(t,offset,p+header,length-header);
}
/* Capability handlers accept validated API envelopes. They do not select
 * plugins or implement server content policy. Pending display IO completes in
 * poll_display/poll_video and emits a separate completion fact. */
static pm_api_result api_welcome(uint8_t kind,const char *j){
    (void)kind;if(state!=SESSION)return PM_API_IGNORED;

        if(!number(j,"incarnation",&incarnation)||!number(j,"session_epoch",&session_epoch)){
            retry();return PM_API_ERROR;
        }
        if(settings.incarnation!=incarnation){
            uint32_t selected=settings.profile;defaults();settings.profile=selected;
        }
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
        reset_assembly();
#endif
        retry_ms=settings.retry_s*1000u;
        display_epoch=0;state=ACTIVE;last_heartbeat_ms=now_ms();
#if PM_DIAGNOSTIC_STAGE
        pm_logf("diag stage=welcome session=%llu\n",(unsigned long long)session_epoch);
#endif
        if(!retained_pixels){pm_display_status("PICO MADO","WAIT IMAGE");}return PM_API_COMPLETED;

}
static pm_api_result api_settings(uint8_t kind,const char *j){
    (void)kind;if(state!=ACTIVE)return PM_API_IGNORED;
    if(!settings_message(j)){retry();return PM_API_ERROR;}return PM_API_COMPLETED;
}
static pm_api_result api_input_result(uint8_t kind,const char *j){
    if(state!=ACTIVE)return PM_API_IGNORED;

        uint64_t epoch=display_epoch,reply_inc,reply_session;
        bool contextual=strstr(j,"\"input_context\"")!=NULL;
        bool has_epoch=number(j,"display_epoch",&epoch);
        if((contextual && (!has_epoch || !number(j,"incarnation",&reply_inc) ||
             !number(j,"session_epoch",&reply_session) || reply_inc!=incarnation || reply_session!=session_epoch)) ||
           (kind==10 && !has_epoch)){
            pm_logf("net reply kind=%u current=%llu result=stale\n",kind,(unsigned long long)display_epoch);return PM_API_REJECTED;
        }
        if(pm_switch_reply(&switching,epoch,has_epoch))
            pm_display_feedback(PM_DISPLAY_FEEDBACK_RESTORE);
        prioritize_received_image();
        if(has_epoch && epoch<display_epoch)return PM_API_REJECTED;
        // A matching reply may arrive after FrameStart or between its chunks.
        // Only a genuinely newer generation invalidates an assembly.
        bool keep=assembly.active && (!has_epoch || epoch==display_epoch);
        if(has_epoch && epoch>display_epoch){display_epoch=epoch;reset_assembly();}
        await_selection=false;
        if(string_equals(j,"mode","normal"))sub_mode=false;
        else if(string_equals(j,"mode","sub"))sub_mode=true;
        else if(kind==17)sub_mode=!pending_sub_cancel;
        else if(pending_sub_cancel)sub_mode=false;
        pending_sub_cancel=false;
        pm_logf("net reply kind=%u epoch=%llu keep=%u\n",kind,(unsigned long long)display_epoch,keep?1u:0u);
        if(kind==10 && pending_standby)standby();
        return PM_API_COMPLETED;

}
static pm_api_result api_error(uint8_t kind,const char *j){
    (void)kind;if(state!=ACTIVE)return PM_API_IGNORED;

        // Only fixed protocol error names are printable; never echo received JSON.
        static const char *const codes[]={"stale_frame","invalid_ack","invalid_ack_token","invalid_input",
            "invalid_frame","invalid_hello","stale_session","stale_settings",
            "unavailable","hello_required","duplicate_hello","unexpected_message"};
        const char *code="unknown";
        for(size_t i=0;i<sizeof codes/sizeof codes[0];i++)
            if(string_equals(j,"code",codes[i])){code=codes[i];break;}
        pm_logf("net profile=%s server_error=%s\n",profile_name(),code);
        retry();return PM_API_ERROR;

}
static pm_api_result api_frame(uint8_t kind,const char *j){
    if(state!=ACTIVE)return PM_API_IGNORED;
    pm_token t;if(!token_from(j,&t))return PM_API_REJECTED;
    if(t.incarnation!=incarnation || t.session_epoch!=session_epoch ||
       t.screen_owner_epoch!=owner_epoch || t.display_epoch<display_epoch)return PM_API_REJECTED;
    // A validated newer FrameStart is also proof that the server accepted
    // the selection, even when its control reply is still in flight.
    if(await_selection && (kind!=12 || t.display_epoch<=display_epoch || pending_standby))return PM_API_REJECTED;
    if(kind==12){
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
        unsigned refresh_minutes=0;uint64_t refresh_minute=0;
        if(strstr(j,"\"eink_refresh\"")){
            uint64_t interval;
            if(!number(j,"full_refresh_minutes",&interval) || interval<1 || interval>60 ||
               !number(j,"minute",&refresh_minute))return PM_API_REJECTED;
            refresh_minutes=(unsigned)interval;
        }
#endif
        uint64_t w=0,h=0,len=0,crc=0;
        bool header=number(j,"width",&w) && number(j,"height",&h) &&
            number(j,"byte_len",&len) && number(j,"crc32",&crc) && string_equals(j,"format",PM_PIXEL_FORMAT_NAME);
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
        if(!header || !pm_frame_header_valid(w,h,PM_PIXEL_FORMAT_NAME,len,crc))return PM_API_REJECTED;
        // A known replay is not a new image and cannot evict a distinct latest
        // pending frame, including pixels still being assembled.
        if(((drawing && pm_token_equal(drawing_token,t)) ||
            (have_display && pm_token_equal(last_rendered,t))) &&
           pending_frame.phase!=PM_EINK_EMPTY && !pm_token_equal(pending_frame.frame.token,t))
            return PM_API_REJECTED;
        reset_assembly();
#endif
#if PM_DISPLAY_TYPE == PM_DISPLAY_LCD
        if(lcd_waiting_frame)pm_switch_forget(&switching,lcd_waiting_token);
        lcd_waiting_frame=false;
#endif
        if(!header || !pm_start(&assembly,t,w,h,PM_PIXEL_FORMAT_NAME,len,crc))reset_assembly();
        else {
#if PM_DISPLAY_TYPE == PM_DISPLAY_LCD
            lcd_waiting_frame=false;receiving_stream=false;
            if(strstr(j,"\"stream\"")){
                uint64_t fps,sequence,generation;
                if(!number(j,"fps",&fps) || fps<1 || fps>30 || !number(j,"sequence",&sequence)
                    || !number(j,"generation",&generation)){reset_assembly();return PM_API_REJECTED;}
                receiving_stream=true;receiving_sequence=sequence;
                pm_video_prepare(&video,t,(uint32_t)fps,generation,video_now_us());
                pm_diag_frame_begin(t.session_epoch,t.frame_id,sequence);
            }else pm_video_reset(&video);
#endif
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
            pm_eink_pending_start(&pending_frame,t,refresh_minutes,refresh_minute);
#endif
            display_epoch=t.display_epoch;await_selection=false;
            // Web changes can leave SUB without a physical button response.
            // Only a validated current frame may replace the cached mode.
            if(string_equals(j,"mode","normal"))sub_mode=false;
            else if(string_equals(j,"mode","sub"))sub_mode=true;
            pm_trace_event(PM_TRACE_FRAME_START,t.frame_id,0,0);
            #if PM_DISPLAY_TYPE == PM_DISPLAY_LCD
            if(!receiving_stream)
#endif
            pm_ack_probe_note(PM_AP_FRAME,0,t.session_epoch,t.frame_id,(uint32_t)t.display_epoch,(uint32_t)(t.display_epoch>>32),(uint32_t)t.screen_owner_epoch,(uint32_t)(t.screen_owner_epoch>>32));
            pm_observe(PM_OB_FRAME,1,PM_OB_RECEIVED,0,observed_parent(j),t.session_epoch,t.frame_id,(uint32_t)t.display_epoch,(uint32_t)(t.display_epoch>>32));
            pm_net_rx_frame_start(t.frame_id,t.session_epoch);
        }
        return assembly.active?PM_API_PENDING:PM_API_REJECTED;
    }
    uint64_t offset;size_t count;
    if(number(j,"offset",&offset) && parse_bytes(j,&count))return receive_chunk(t,offset,fragment,count);
    return PM_API_REJECTED;
}
static const pm_device_api device_apis[]={
    {2,"welcome",api_welcome},{3,"settings",api_settings},
    {10,"input_accepted",api_input_result},{11,"error",api_error},
    {12,"frame_start",api_frame},{13,"frame_chunk",api_frame},
    {17,"plugin_position",api_input_result}
};
static void process(uint8_t kind,const char *j){
    uint64_t parent=0,requested_frame=0;
#if PM_EVENT_STREAM
    parent=observed_parent(j);
    if(kind==12 || kind==13)(void)number(j,"frame_id",&requested_frame);
#endif
    uint64_t op=pm_observe(PM_OB_API,kind,PM_OB_RECEIVED,0,parent,session_epoch,requested_frame,(uint32_t)state,0);
#if PM_EVENT_STREAM
    pm_net_observed_api(op,requested_frame);
#endif
    const pm_device_api *api=pm_device_api_find(device_apis,sizeof device_apis/sizeof device_apis[0],kind);
    if(!api || !string_equals(j,"type",api->type)){
        pm_observe(PM_OB_API,kind,PM_OB_REJECTED,op,parent,session_epoch,requested_frame,1,0);retry();return;
    }
    pm_observe(PM_OB_API,kind,PM_OB_DISPATCHED,op,parent,session_epoch,requested_frame,(uint32_t)state,0);
    pm_api_result result=api->handle(kind,j);
    static const uint8_t phases[]={PM_OB_REJECTED,PM_OB_REJECTED,PM_OB_PENDING,PM_OB_COMPLETED,PM_OB_ERROR,PM_OB_CANCELLED};
    pm_observe(PM_OB_API,kind,phases[result],op,parent,session_epoch,requested_frame,(uint32_t)state,result);
}
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
static void poll_display(void){
    pm_display_poll();
    bool drawn;
    if(pm_display_take_result(&drawn)){
        drawing=false;
        bool current=state==ACTIVE && !await_selection && !panel_standby &&
            pm_token_current(drawing_token,incarnation,session_epoch,display_epoch,owner_epoch);
        pm_observe(PM_OB_DISPLAY,1,drawn?PM_OB_COMPLETED:PM_OB_ERROR,0,0,drawing_token.session_epoch,drawing_token.frame_id,current,0);
        if(drawn && !current)pm_observe(PM_OB_FRAME,1,PM_OB_CANCELLED,0,0,drawing_token.session_epoch,drawing_token.frame_id,0,0);
        if(drawn && state==ACTIVE && !await_selection && !panel_standby &&
           pm_token_current(drawing_token,incarnation,session_epoch,display_epoch,owner_epoch)){
            have_display=true;last_rendered=drawing_token;
            pm_switch_complete(&switching,drawing_token);
            if(pending_render_ack)pm_ack_probe_note(PM_AP_PENDING_REPLACED,9,pending_render_token.session_epoch,pending_render_token.frame_id,(uint32_t)drawing_token.frame_id,(uint32_t)(drawing_token.frame_id>>32),0,0);
            pending_render_token=drawing_token;pending_render_ack=true;
            pm_ack_probe_note(PM_AP_DRAW_END,0,drawing_token.session_epoch,drawing_token.frame_id,drawn,0,0,0);
            pm_ack_probe_note(PM_AP_PENDING_SET,9,drawing_token.session_epoch,drawing_token.frame_id,0,0,0,0);
        }
    }
    if(pending_frame.phase==PM_EINK_READY)(void)dispatch_pending_eink();
    if(panel_standby && !pm_display_busy()){panel_standby=false;standby();}
}
#endif
static bool apply_local_input(pm_input_transition t,uint64_t accepted_us){
    if(t.clear_retained)retained_pixels=false;
    switch(t.action){
    case PM_INPUT_WAKE:start_wifi();break;
    case PM_INPUT_MENU_MOVE:
        if(profile_ready(menu_profile^1)){
            menu_profile^=1;pm_display_status("NETWORK",menu_profile?"AWAY":"HOME");
        }else pm_display_status("AWAY","UNSET");
        break;
    case PM_INPUT_MENU_ACCEPT:
        active_profile=menu_profile;settings.profile=active_profile;store_settings();
        pm_logf("net profile=%s selected\n",profile_name());start_wifi();break;
    case PM_INPUT_MENU:menu();break;
    case PM_INPUT_STANDBY:standby();break;
    case PM_INPUT_POWER:
        /* Completion is fed back before applying post-send local state. */
        if(!send_button_input(7,"power",accepted_us)){retry();return false;}
        if(sub_mode){pending_sub_cancel=true;await_selection=true;reset_assembly();}
        else {pending_standby=true;await_selection=true;started_ms=now_ms();reset_assembly();}
        break;
    case PM_INPUT_SUB:return send_button_input(14,"enter_sub",accepted_us);
    case PM_INPUT_UP:case PM_INPUT_DOWN:
        if(t.await_before_send){await_selection=true;reset_assembly();}
        return send_button_input(t.action==PM_INPUT_UP?15:16,t.action==PM_INPUT_UP?"up":"down",accepted_us);
    case PM_INPUT_IGNORE:return false;
    }
    return true;
}
static void poll_switch_feedback(void){
    if(state!=ACTIVE || pending_standby){
        if(switching.waiting || switching.hint_requested)
            pm_display_feedback(PM_DISPLAY_FEEDBACK_REPLACE);
        pm_switch_cancel(&switching);return;
    }
    prioritize_received_image();
#if PM_DISPLAY_TYPE == PM_DISPLAY_LCD
    if(lcd_waiting_frame && pm_switch_eligible(&switching,lcd_waiting_token))
        (void)finish_lcd_image(lcd_waiting_token);
#endif
    if(pm_switch_request_hint(&switching,time_us_64()))
        pm_display_feedback(PM_DISPLAY_FEEDBACK_SHOW);
}
#ifndef PM_INPUT_OPERATIONS_MASK
#define PM_INPUT_OPERATIONS_MASK PM_OPERATION_MASK
#endif
_Static_assert((PM_INPUT_OPERATIONS_MASK&~PM_OPERATION_MASK)==0,"unsupported operation capability bits");
/* This is the one application operation API. Adapters supply a completed
 * semantic operation; only GPIO/SIM adapters interpret physical gestures. */
static pm_input_result perform_operation(pm_operation operation,unsigned origin,bool long_press){
    bool blocked=false;
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
    blocked=panel_standby;
#endif
    pm_input_plan plan=pm_input_decide((pm_input_view){state,sub_mode,PM_INPUT_OPERATIONS_MASK,blocked},operation);
    if(plan.result!=PM_INPUT_READY)return plan.result;
    unsigned i=pm_operation_index(operation);pm_input_transition t=plan.transition;
    uint64_t op=pm_observe(PM_OB_INPUT,(uint16_t)i,PM_OB_RECEIVED,0,0,session_epoch,0,(uint32_t)state,(long_press?1u:0u)|(origin<<1));
    pm_observe(PM_OB_INPUT,(uint16_t)i,PM_OB_DISPATCHED,op,0,session_epoch,0,t.action,t.await_before_send);
#if PM_EVENT_STREAM
    uint64_t previous=input_operation;input_operation=op;
#endif
    bool ok=apply_local_input(t,time_us_64());
#if PM_EVENT_STREAM
    input_operation=previous;
#endif
    pm_observe(PM_OB_INPUT,(uint16_t)i,ok?PM_OB_COMPLETED:PM_OB_ERROR,op,0,session_epoch,0,(uint32_t)state,t.action);
    return ok?PM_INPUT_DISPATCHED:PM_INPUT_FAILED;
}
static pm_input_result usb_operation(pm_operation operation){return perform_operation(operation,2,false);}
static void button_with_origin(unsigned i,bool long_press,unsigned origin){
    pm_operation operation;if(pm_operation_from_index(i,&operation))(void)perform_operation(operation,origin,long_press);
}
static inline void button(unsigned i,bool long_press){button_with_origin(i,long_press,0);}
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
static void poll_local_inputs(uint32_t now,bool allowed){
    bool connected=stdio_usb_connected();
    /* Gate must open before a committed detached simulator can run. */
    pm_ack_probe_usb_state(connected);
    /* Timestamp the GPIO scan after display work, not the pre-display loop. */
    now=now_ms();
    bool raw[4]={false};unsigned physical=0;
    for(unsigned i=0;i<input_count();i++){
        raw[i]=!gpio_get(button_pins[i]);
        if(raw[i] || buttons[i].stable)physical|=1u<<i;
    }
    pm_sim_dispatch synthetic=pm_usb_control_tick(connected,physical,allowed,session_epoch,state,state==STANDBY,settings.long_ms,now);
    if(!allowed)return;
    for(unsigned i=0;i<input_count();i++){
        bool was_pressed=buttons[i].stable;
        pm_operation operation;bool long_press;
        bool event=pm_button_sampled_binding_step(&buttons[i],pm_button_binding_at(PM_BUTTON_COUNT,i),raw[i],now,
            settings.long_ms,state==STANDBY,&operation,&long_press);
        if(!was_pressed && buttons[i].stable)pm_trace_event(PM_TRACE_BUTTON,0,(int32_t)i,(uint32_t)state);
        if(event){
            if(state==ACTIVE)pm_ack_probe_button(session_epoch,display_epoch,owner_epoch,pm_operation_index(operation),long_press);
            (void)perform_operation(operation,0,long_press);
        }
    }
    if(synthetic.valid){
        if(state==ACTIVE)pm_ack_probe_sim_button(session_epoch,display_epoch,owner_epoch,synthetic.logical,synthetic.long_press,synthetic.job,synthetic.step);
        button_with_origin(synthetic.logical,synthetic.long_press,1);
        pm_usb_control_dispatched(synthetic,session_epoch,state,now_ms());
    }
}
static void finish_loop(uint64_t started){
    pm_trace_flush();
#if PM_EVENT_STREAM
    pm_observation_drain(4,768,500);
#endif
    pm_trace_slow(PM_TRACE_SLOW_LOOP,started,(uint32_t)state);
    pm_stats_note(PM_STAT_LOOP,started);
}
int main(void){
    stdio_init_all();
    uint64_t control_boot=((uint64_t)get_rand_32()<<32)|get_rand_32();
    pm_ack_probe_init_boot(control_boot);
    pm_usb_control_init(control_boot,input_count());
    pm_usb_control_set_operations(PM_INPUT_OPERATIONS_MASK,usb_operation);
    bool net_ok=pm_net_init();
#if PM_AWAY_EEX1
    pm_endpoint_client_init(&endpoint_client,pm_endpoint_sdk_io());
    endpoint_client.discovery_port=PM_EEX1_PORT;endpoint_initialized=true;
#endif
    if(net_ok){cyw43_arch_enable_sta_mode();wifi_enabled=true;}
    // Match the working YouTube order: CYW43 power/PIO first, panel reset next.
    pm_display_init();
    load_settings();active_profile=settings.profile;
    retry_ms=settings.retry_s*1000u;
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
        pm_stats_loop_begin(loop_started);
        uint32_t now=now_ms();
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
        bool draining=panel_standby;
        poll_display();
        if(draining){
            poll_local_inputs(now,false);log_usb_snapshot();
            finish_loop(loop_started);sleep_ms(5);continue;
        }
#endif
        poll_local_inputs(now,true);
        log_usb_snapshot();
#if PM_AWAY_EEX1
        poll_endpoint();
#endif
        if(state==STANDBY){finish_loop(loop_started);sleep_ms(5);continue;}
        uint64_t poll_started=pm_trace_clock();
        cyw43_arch_poll();
        pm_trace_slow(PM_TRACE_SLOW_POLL,poll_started,1);
        // Button handling may save settings/draw and start a newer attempt.
        // Refresh before subtracting started_ms; the pre-button clock can wrap.
        now=now_ms();
        if(state==WIFI){
            log_wifi_progress();
            if(cyw43_tcpip_link_status(&cyw43_state,CYW43_ITF_STA)==CYW43_LINK_UP){
                check_wifi_pm();
                apply_radio_defaults();
#if PM_DIAGNOSTIC_STAGE
                pm_display_reference_pattern();
                pm_logf("%s\n","diag stage=wifi_up_pattern duration_ms=6000");
                diagnostic_hold(6000,true);
#endif
                start_server();
            }else if(now-started_ms>settings.wifi_s*1000u)retry_recovery();
        }else if(state==SERVER||state==SESSION||state==ACTIVE){
            if(cyw43_tcpip_link_status(&cyw43_state,CYW43_ITF_STA)!=CYW43_LINK_UP){
#if PM_DISPLAY_TYPE == PM_DISPLAY_LCD
                if(recover_transport_loss()){pm_stats_note(PM_STAT_LOOP,loop_started);continue;}
#endif
                start_wifi();pm_stats_note(PM_STAT_LOOP,loop_started);continue;
            }
            pm_net_poll();
#if PM_AWAY_EEX1
            endpoint_proxy_progress();
#endif
            if(recover_transport_failure()){
                pm_stats_note(PM_STAT_LOOP,loop_started);continue;
            }
            pm_net_stage transport=pm_net_state();
            bool peer_eof=transport==PM_NET_DRAINING || transport==PM_NET_DRAINED;
#if PM_DISPLAY_TYPE == PM_DISPLAY_LCD
            // Only an established application may salvage EOF data. A new
            // handshake never consumes bytes in an old session context.
            if(peer_eof && state!=ACTIVE){retry_recovery();pm_stats_note(PM_STAT_LOOP,loop_started);continue;}
#else
            // Preserve non-LCD disconnect behavior.
            if(peer_eof){retry();pm_stats_note(PM_STAT_LOOP,loop_started);continue;}
#endif
            if(state==SERVER && pm_net_state()==PM_NET_READY){
                if(!send_hello()){if(!recover_transport_failure())retry();pm_stats_note(PM_STAT_LOOP,loop_started);continue;}state=SESSION;
            }
            if(state!=ACTIVE && now-started_ms>settings.app_s*1000u){
#if PM_AWAY_EEX1
                endpoint_result(PM_EEX_FAIL,PM_EEX_TIMEOUT);
#endif
                retry_recovery();pm_stats_note(PM_STAT_LOOP,loop_started);continue;}
            uint8_t kind;
            // Bound cooperative receive work so image decoding cannot starve
            // network polling or physical input under continuous playback.
            unsigned budget=video_mode()?8u:2u;
            for(unsigned messages=0;messages<budget && pm_net_take(&kind,json,sizeof json);messages++){
                if(recover_transport_failure())break;
                uint64_t process_started=pm_stats_clock();
                if(kind==18)process_frame_bytes((const uint8_t *)json,pm_net_payload_length());
                else process(kind,json);
                pm_stats_note(PM_STAT_PROCESS,process_started);
#if PM_EVENT_STREAM
                pm_observation_drain(6,768,200);
#endif
                if(state==ACTIVE){
                    pm_net_poll();
                    if(recover_transport_failure())break;
                }
            }
            // A false take can consume time in Close/pong output. Recheck its
            // deadline as well as decoder failures before starting any draw.
            if(recover_transport_failure()){pm_stats_note(PM_STAT_LOOP,loop_started);continue;}
            // Welcome/settings handling may also advance timers and draw.
            now=now_ms();
            if(!send_pending_render_ack()){if(!recover_transport_failure())retry();pm_stats_note(PM_STAT_LOOP,loop_started);continue;}
            poll_video();
#if PM_DISPLAY_TYPE == PM_DISPLAY_LCD
            if(state==ACTIVE && pm_net_state()==PM_NET_DRAINED &&
               (!video.ready || await_selection || pending_standby)){
                retry_eof();pm_stats_note(PM_STAT_LOOP,loop_started);continue;
            }
#endif
            if(state==ACTIVE && pending_standby && now-started_ms>2000)standby();
            if(state==ACTIVE && pm_net_state()==PM_NET_READY && now-last_heartbeat_ms>=settings.heartbeat_s*1000u){
                heartbeat();last_heartbeat_ms=now;
            }
        }else if(state==RETRY){
            if(wifi_enabled && cyw43_tcpip_link_status(&cyw43_state,CYW43_ITF_STA)!=CYW43_LINK_UP){
                wifi_off();connection_status("WIFI","RETRY");
            }
            if(now-started_ms>=retry_ms){
                retry_ms*=2;if(retry_ms>settings.retry_max_s*1000u)retry_ms=settings.retry_max_s*1000u;
                if(wifi_enabled)start_server();else start_wifi();
            }
        }
        poll_switch_feedback();
        finish_loop(loop_started);sleep_ms(state==ACTIVE && video_mode()?1u:5u);
    }
}
