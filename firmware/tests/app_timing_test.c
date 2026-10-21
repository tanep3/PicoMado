// Execute the production main loop, with hardware and network I/O mocked.
// GPIO holds last at least the real 35ms debounce; clock jumps cover deadlines.
#include <assert.h>
#include <setjmp.h>
#include <stdio.h>
#include <string.h>
#ifndef PM_APP_MAIN_SOURCE
#define PM_APP_MAIN_SOURCE "../src/main.c"
#endif
#define main pm_firmware_entry
#include PM_APP_MAIN_SOURCE
#undef main
#include "normal_usb_stubs.h"

uint8_t test_flash_sector[4096];
cyw43_t cyw43_state;
static jmp_buf finished;
static uint32_t clock_ms=1000,confirm_loop_ms;
static int pressed=-1,wifi_link=CYW43_LINK_DOWN,ip_link=CYW43_LINK_DOWN;
static int sdk_result,pm_result,last_pm_result;
static int pm_read_result;
static uint32_t pm_read_value=CYW43_NONE_PM;
static unsigned lwip_depth,pm_calls,pm_reads;
static unsigned joins,erases,server_begins,hello_count,heartbeat_count,loops;
static bool sta_on,server_ready,welcome_available;
static bool usb_connected;
static bool drop_during_retry;
uint64_t time_us_64(void){return (uint64_t)clock_ms*1000;}
#if PM_TRACE_ENABLE || defined(PM_TEST_REAL_USB_CONTROL)
static bool trace_output;static char trace_text[32768];static size_t trace_used;
static size_t trace_write_cap=SIZE_MAX;
size_t pm_trace_usb_write(const char *p,size_t n){
    if(!trace_output)return 0;
    if(n>trace_write_cap)n=trace_write_cap;
    assert(trace_used+n<sizeof trace_text);
    memcpy(trace_text+trace_used,p,n);trace_used+=n;trace_text[trace_used]=0;return n;
}
#endif
static unsigned wake_trial,wake_loops,frame_receipts,frame_renders,stream_status_count;
static char stream_status_last[768];
static bool checking_frame;
static bool net_send_rejected,display_frame_rejected;
static const unsigned ignored_wake_buttons[]={0,1,3};
static const char *last_lcd_one,*last_lcd_two;
static pm_net_stage network_stage=PM_NET_DOWN;
static enum {
    INITIAL,OPEN_AWAY_MENU,SELECT_AWAY,CONFIRM_AWAY,WIFI_BOUNDARY,WIFI_TIMEOUT,
    RETRY_BEFORE,RETRY_EXACT,CANCEL_MENU,CANCEL_SELECT,CANCEL_CONFIRM,
    HOME_MENU,HOME_SELECT,HOME_CONFIRM,HOME_SERVER,SERVER_BOUNDARY,
    SERVER_TIMEOUT,HOME_RETRY,HOME_CONNECT,HOME_ACTIVE,WAKE
} phase;

absolute_time_t get_absolute_time(void){return clock_ms;}
uint32_t to_ms_since_boot(absolute_time_t t){return (uint32_t)t;}
void stdio_init_all(void){}
bool stdio_usb_connected(void){return usb_connected;}
void gpio_init(uint pin){assert(pin>=12 && pin<=15);}
void gpio_set_dir(uint pin,int dir){assert(pin>=12 && pin<=15 && dir==GPIO_IN);}
void gpio_pull_up(uint pin){assert(pin>=12 && pin<=15);}
bool gpio_get(uint pin){
#if PM_BUTTON_COUNT == 4
    if(pin==PM_BUTTON_SUB_PIN && phase==CONFIRM_AWAY)confirm_loop_ms=clock_ms;
#endif
    return pressed<0 || pin!=button_pins[pressed];
}
void pico_get_unique_board_id_string(char *id,size_t n){
    assert(n>=17);memcpy(id,PM_EXPECTED_BOARD_ID,17);
}
uint32_t save_and_disable_interrupts(void){return 0;}
void restore_interrupts(uint32_t status){assert(status==0);}
void flash_range_erase(uint32_t offset,size_t n){
    assert(offset==PICO_FLASH_SIZE_BYTES-FLASH_SECTOR_SIZE && n==4096);
    memset(test_flash_sector,0xff,n);erases++;clock_ms+=5;
}
void flash_range_program(uint32_t offset,const uint8_t *p,size_t n){
    assert(offset==PICO_FLASH_SIZE_BYTES-FLASH_SECTOR_SIZE && n==256);
    memcpy(test_flash_sector,p,n);clock_ms+=2;
}
const char *pm_display_capability(void){return PM_DISPLAY_CAPABILITY;}
static unsigned feedback_shows,feedback_restores,feedback_replaces,input_sends;
void pm_display_init(void){}
void pm_display_feedback(pm_display_feedback_action a){
    if(a==PM_DISPLAY_FEEDBACK_SHOW){assert(input_sends);feedback_shows++;}
    else if(a==PM_DISPLAY_FEEDBACK_RESTORE)feedback_restores++;
    else feedback_replaces++;
}
void pm_display_status(const char *a,const char *b){
    assert(a && b);last_lcd_one=a;last_lcd_two=b;clock_ms+=20;
}
bool pm_display_frame(uint8_t *p,size_t n){
    assert(p && n==PM_IMAGE_BYTES);
    if(display_frame_rejected)return false;
    if(checking_frame){
        assert(frame_receipts>frame_renders);
        pm_trace_event(PM_TRACE_SPI_BEGIN,pm_trace_current_frame(),0,0);clock_ms+=14;
        pm_trace_event(PM_TRACE_SPI_END,pm_trace_current_frame(),PM_IMAGE_BYTES,15625000);
    }
    return true;
}
void pm_display_standby(void){}
void pm_display_wake(void){}
void pm_display_reference_pattern(void){assert(!"normal build must not draw diagnostics");}
bool pm_net_init(void){return true;}
void pm_net_close(void){network_stage=PM_NET_DOWN;}
void pm_net_begin(const pm_profile *p){
#if PM_AWAY_EEX1
    if(p->socks){
        assert(p==&endpoint_profile && active_profile==1);
        assert(p->ssid==profiles[1].ssid && p->password==profiles[1].password);
        assert(p->proxy_user==profiles[1].proxy_user && p->proxy_password==profiles[1].proxy_password);
        assert(p->server_ip==profiles[1].server_ip && p->proxy_port>0);
    }else
#endif
    {assert(p==&profiles[0] && !p->socks);}
    server_begins++;network_stage=PM_NET_TCP;
}
void pm_net_poll(void){if(server_ready)network_stage=PM_NET_READY;}
pm_net_stage pm_net_state(void){return network_stage;}
pm_net_stage pm_net_checkpoint(void){return network_stage;}
#if !PM_AWAY_EEX1
pm_net_failure pm_net_last_failure(void){return PM_NET_FAIL_PROTOCOL;}
#endif
#ifdef PM_CAUSAL_FIXTURE
static void (*wire_observer)(uint8_t,const char *);
#endif
bool pm_net_send(uint8_t kind,const char *j){
    assert(j && network_stage==PM_NET_READY);
    if(net_send_rejected)return false;
#ifdef PM_CAUSAL_FIXTURE
    if(wire_observer)wire_observer(kind,j);
#endif
    if(kind==7 || kind==14 || kind==15 || kind==16)input_sends++;
    if(kind==1){hello_count++;welcome_available=true;}
    if(kind==5)heartbeat_count++;
    if(kind==8){frame_receipts++;clock_ms++;}
    if(kind==9){frame_renders++;clock_ms++;}
    if(kind==19){stream_status_count++;snprintf(stream_status_last,sizeof stream_status_last,"%s",j);}
    return true;
}
bool pm_net_send_optional(uint8_t kind,const char *j){return pm_net_send(kind,j);}
#if PM_EVENT_STREAM
bool pm_net_send_observed(uint8_t k,const char *j,uint64_t n,uint64_t p,uint64_t s){(void)n;(void)p;(void)s;return pm_net_send(k,j);}
void pm_net_observed_api(uint64_t operation,uint64_t frame){(void)operation;(void)frame;}
#endif
size_t pm_net_payload_length(void){return 0;}
bool pm_net_take(uint8_t *kind,char *j,size_t n){
    if(!welcome_available)return false;
    const char welcome[]="{\"type\":\"welcome\",\"data\":{\"incarnation\":1,\"session_epoch\":1}}";
    assert(n>sizeof welcome);memcpy(j,welcome,sizeof welcome);*kind=2;
    welcome_available=false;return true;
}
void cyw43_arch_enable_sta_mode(void){sta_on=true;}
void cyw43_arch_disable_sta_mode(void){sta_on=false;wifi_link=ip_link=CYW43_LINK_DOWN;}
void cyw43_arch_lwip_begin(void){assert(lwip_depth==0);lwip_depth=1;}
void cyw43_arch_lwip_end(void){assert(lwip_depth==1);lwip_depth=0;}
int cyw43_wifi_pm(cyw43_t *s,uint32_t value){
    assert(sta_on && lwip_depth==1 && s==&cyw43_state && value==CYW43_NONE_PM);
    pm_calls++;last_pm_result=pm_result;return pm_result;
}
int cyw43_wifi_get_pm(cyw43_t *s,uint32_t *value){
    assert(sta_on && ip_link==CYW43_LINK_UP && lwip_depth==1 && s==&cyw43_state);
    pm_reads++;
    if(!pm_read_result)*value=pm_read_value;
    return pm_read_result;
}
void cyw43_arch_poll(void){}
int cyw43_arch_wifi_connect_async(const char *ssid,const char *pw,uint32_t auth){
    assert(sta_on && wifi_enabled && auth==CYW43_AUTH_WPA2_AES_PSK);
    assert(ssid==profiles[active_profile].ssid && pw==profiles[active_profile].password);
    // Covers the real boot path where main already enabled STA before LCD init.
    assert(pm_calls==joins+1 && wifi_pm_status==WIFI_PM_PENDING);
    joins++;wifi_link=ip_link=CYW43_LINK_JOIN;return sdk_result;
}
int cyw43_wifi_link_status(cyw43_t *s,int i){assert(s==&cyw43_state && i==0);return wifi_link;}
int cyw43_tcpip_link_status(cyw43_t *s,int i){assert(s==&cyw43_state && i==0);return ip_link;}

static bool released(void){
    for(unsigned i=0;i<input_count();i++)if(buttons[i].raw || buttons[i].stable)return false;
    return true;
}
static void press_after_release(int index){if(pressed<0 && released())pressed=index;}
static void check_menu(void){
    assert(state==MENU && !sta_on && !wifi_enabled && network_stage==PM_NET_DOWN);
    assert(!assembly.active && !pending_render_ack);
}
void sleep_ms(unsigned ms){
    assert(ms==5 && ++loops<1000);
    if(sdk_result){
        assert(state==RETRY && joins==1);puts("SDK failure: RETRY preserved");longjmp(finished,1);
    }
    switch(phase){
    case INITIAL:
        assert(state==WIFI && joins==1 && active_profile==0);
        assert(pm_calls==1 && pm_reads==0);
        assembly.active=true;pending_render_ack=true;pressed=0;phase=OPEN_AWAY_MENU;break;
    case OPEN_AWAY_MENU:
        if(state==MENU){check_menu();pressed=-1;phase=SELECT_AWAY;}break;
    case SELECT_AWAY:
        press_after_release(1);
        if(menu_profile==1){assert(active_profile==0);pressed=-1;phase=CONFIRM_AWAY;}break;
    case CONFIRM_AWAY:
        press_after_release(0);
        if(joins==2){
            assert(started_ms>confirm_loop_ms && state==WIFI && active_profile==1);
            assert(settings.profile==1 && erases==1);
            printf("regression: AWAY start %lu > button-loop clock %lu; no immediate timeout\n",
                (unsigned long)started_ms,(unsigned long)confirm_loop_ms);
            pressed=-1;clock_ms=started_ms+settings.wifi_s*1000u;phase=WIFI_BOUNDARY;return;
        }break;
    case WIFI_BOUNDARY:
        assert(state==WIFI);clock_ms++;phase=WIFI_TIMEOUT;return;
    case WIFI_TIMEOUT:
        assert(state==RETRY && !sta_on && joins==2 && retry_ms==5000);
        clock_ms=started_ms+retry_ms-1;phase=RETRY_BEFORE;return;
    case RETRY_BEFORE:
        assert(state==RETRY && joins==2);clock_ms++;phase=RETRY_EXACT;return;
    case RETRY_EXACT:
        assert(state==WIFI && joins==3 && active_profile==1 && retry_ms==10000);
        puts("Wi-Fi timeout boundary and 5s retry/backoff: OK");phase=CANCEL_MENU;break;
    case CANCEL_MENU:
        press_after_release(0);
        if(state==MENU){check_menu();pressed=-1;phase=CANCEL_SELECT;}break;
    case CANCEL_SELECT:
        press_after_release(1);
        if(menu_profile==0){assert(active_profile==1);pressed=-1;phase=CANCEL_CONFIRM;}break;
    case CANCEL_CONFIRM:
        press_after_release(2);
        if(joins==4){
            assert(state==WIFI && active_profile==1 && settings.profile==1 && erases==1);
            puts("menu cancel: original AWAY retained; no immediate timeout");
            pressed=-1;phase=HOME_MENU;
        }break;
    case HOME_MENU:
        press_after_release(0);
        if(state==MENU){check_menu();pressed=-1;phase=HOME_SELECT;}break;
    case HOME_SELECT:
        press_after_release(1);
        if(menu_profile==0){pressed=-1;phase=HOME_CONFIRM;}break;
    case HOME_CONFIRM:
        press_after_release(0);
        if(joins==5){
            assert(state==WIFI && active_profile==0 && settings.profile==0 && erases==2);
            puts("HOME selection: saved, started, no immediate timeout");
            pressed=-1;ip_link=CYW43_LINK_UP;phase=HOME_SERVER;
        }break;
    case HOME_SERVER:
        assert(state==SERVER && server_begins==1);
        assert(pm_reads==1 && wifi_pm_status==WIFI_PM_CHECKED);
        clock_ms=started_ms+settings.app_s*1000u;phase=SERVER_BOUNDARY;return;
    case SERVER_BOUNDARY:
        assert(state==SERVER);clock_ms++;phase=SERVER_TIMEOUT;return;
    case SERVER_TIMEOUT:
        assert(state==RETRY && sta_on && wifi_enabled && joins==5 && network_stage==PM_NET_DOWN);
        assert(!strcmp(last_lcd_one,"SERVER"));
        puts("server timeout: session closed; healthy Wi-Fi/IP retained");
        if(drop_during_retry){
            ip_link=CYW43_LINK_DOWN;clock_ms=started_ms+retry_ms-30;
        }else clock_ms=started_ms+retry_ms;
        phase=HOME_RETRY;return;
    case HOME_RETRY:
        if(drop_during_retry && state==RETRY){
            assert(!sta_on && !wifi_enabled && !strcmp(last_lcd_one,"WIFI") && !strcmp(last_lcd_two,"RETRY"));
            puts("link lost during server retry: Wi-Fi retry label; remaining backoff retained");
            clock_ms=started_ms+retry_ms;return;
        }
        assert(active_profile==0);
        if(drop_during_retry){assert(state==WIFI && joins==6);ip_link=CYW43_LINK_UP;}
        else {assert(state==SERVER && joins==5 && server_begins==2);puts("server retry: no new Wi-Fi join");}
        server_ready=true;phase=HOME_CONNECT;break;
    case HOME_CONNECT:
        if(state==ACTIVE){
            assert(hello_count==1 && heartbeat_count==0 && server_begins==2);
            // A server-only retry must not reapply/read PM or rejoin Wi-Fi.
            assert(pm_reads==(drop_during_retry?2u:1u));
            puts("HOME recovery: Wi-Fi/IP, server, Hello/Welcome; no early heartbeat");
            standby();assert(state==STANDBY && !sta_on);phase=HOME_ACTIVE;
        }break;
    case HOME_ACTIVE:
        if(wake_trial<3){
            if(wake_loops<15){press_after_release((int)ignored_wake_buttons[wake_trial]);wake_loops++;}
            else {pressed=-1;if(released()){wake_trial++;wake_loops=0;}}
            assert(state==STANDBY && !sta_on && joins==(drop_during_retry?6u:5u));break;
        }
        press_after_release(2);
        if(joins==(drop_during_retry?7u:6u)){
            assert(state==WIFI && active_profile==0);
            puts("standby wake: SUB/UP/DOWN ignored; POWER only; no immediate timeout");phase=WAKE;longjmp(finished,1);
        }break;
    case WAKE:assert(!"test already completed");
    }
    clock_ms+=ms;
}
#if PM_TRACE_ENABLE
static void test_pm_diagnostics(void){
    trace_output=true;
    for(unsigned i=0;i<100;i++)pm_trace_flush();
    trace_used=0;trace_text[0]=0;
    // Wake has requested PM again. Read only after join, once per attempt.
    assert(wifi_pm_status==WIFI_PM_PENDING);
    ip_link=CYW43_LINK_UP;
    unsigned before=pm_reads;
    check_wifi_pm();check_wifi_pm();
    assert(pm_reads==before+1 && wifi_pm_status==WIFI_PM_CHECKED && lwip_depth==0);
    assert(wifi_pm_request_result==last_pm_result && wifi_pm_readback_result==pm_read_result);
    if(!pm_read_result)assert(wifi_pm_readback==pm_read_value);
    // Late USB and its 15-second snapshots replay cached results without IOCTLs.
    usb_connected=false;log_usb_snapshot();usb_connected=true;log_usb_snapshot();
    clock_ms+=15000;log_usb_snapshot();
    assert(pm_reads==before+1);
    for(unsigned i=0;i<100;i++)pm_trace_flush();
    if(pm_read_result){
        assert(strstr(trace_text,"readback_sdk_result=-8"));
        assert(!strstr(trace_text,"raw=") && !strstr(trace_text,"none=1"));
    }else if((pm_read_value&15u)==CYW43_NO_POWERSAVE_MODE){
        assert(strstr(trace_text,"mode=0 none=1"));
    }else{
        assert(strstr(trace_text,"mode=2 none=0") && !strstr(trace_text,"none=1"));
    }
    if(last_pm_result)assert(strstr(trace_text,"request_sdk_result=-5"));
    puts("Wi-Fi PM: boot/wake request, once-per-join readback, cached USB, failure/mismatch reporting PASS");
}
static void test_frame_pipeline(void){
    checking_frame=true;trace_used=0;trace_text[0]=0;
    state=ACTIVE;network_stage=PM_NET_READY;await_selection=false;
    incarnation=session_epoch=1;display_epoch=0;owner_epoch=42;
    have_display=pending_render_ack=false;frame_receipts=frame_renders=0;
    uint8_t zeros[PM_IMAGE_BYTES]={0};uint32_t crc=pm_crc32(zeros,sizeof zeros);
    const char *pattern="\"token\":{\"device_id\":\"%s\",\"incarnation\":1,\"session_epoch\":1,\"display_epoch\":1,\"screen_owner_epoch\":42,\"frame_id\":900}";
    char token[320];snprintf(token,sizeof token,pattern,PM_DEVICE_ID);
    snprintf(json,sizeof json,"{\"type\":\"frame_start\",\"data\":{%s,\"mode\":\"normal\",\"width\":160,\"height\":80,\"format\":\"rgb565be\",\"byte_len\":25600,\"crc32\":%lu}}",token,(unsigned long)crc);
    sub_mode=true;process(12,json);assert(!sub_mode);
    for(unsigned offset=0;offset<PM_IMAGE_BYTES;offset+=PM_FRAGMENT_BYTES){
        unsigned n=PM_IMAGE_BYTES-offset;if(n>PM_FRAGMENT_BYTES)n=PM_FRAGMENT_BYTES;
        int prefix=snprintf(json,sizeof json,"{\"type\":\"frame_chunk\",\"data\":{%s,\"offset\":%u,\"bytes\":[",token,offset);
        assert(prefix>0);size_t k=(size_t)prefix;
        for(unsigned i=0;i<n;i++){json[k++]='0';if(i+1<n)json[k++]=',';}
        memcpy(json+k,"]}}",4);process(13,json);
        if(offset+n<PM_IMAGE_BYTES)assert(!frame_receipts && !frame_renders && !have_display);
    }
    assert(frame_receipts==1 && !frame_renders && have_display && pending_render_ack);
    assert(send_pending_render_ack() && frame_renders==1 && !pending_render_ack);
    assert(send_pending_render_ack() && frame_renders==1);
#if PM_TRACE_STATS
    pm_rx_capture(time_us_64());pm_rx_record measured;
    assert(pm_rx_snapshot_record(0,&measured) && measured.frame==900 && measured.reason==PM_RX_COMPLETE && measured.assembled==PM_IMAGE_BYTES);
#endif
    for(unsigned i=0;i<100;i++)pm_trace_flush();
    const char *events[]={"event=crc_ok","event=receipt_begin","event=receipt_end", "event=spi_begin","event=spi_end","event=render_begin","event=render_end"};
    const char *last=trace_text;
    for(unsigned i=0;i<sizeof events/sizeof events[0];i++){
        const char *next=strstr(last,events[i]);assert(next);last=next+strlen(events[i]);
    }
    assert(strstr(trace_text,"frame=900"));
    // Cancellation/owner change between completed draw and ACK drops stale ACK.
    pending_render_token=last_rendered;pending_render_ack=true;owner_epoch++;
    assert(send_pending_render_ack() && !pending_render_ack && frame_renders==1);
    checking_frame=false;
    puts("frame trace: production parse/CRC -> receipt -> SPI -> rendered; stale completion ACK rejected PASS");
}
#endif
#ifndef PM_APP_TEST_ENTRY
#define PM_APP_TEST_ENTRY main
#endif
int PM_APP_TEST_ENTRY(int argc,char **argv){
    assert(argc==1 || (argc==2 && (!strcmp(argv[1],"sdk-error") ||
        !strcmp(argv[1],"usb-snapshot") || !strcmp(argv[1],"error-log") ||
        !strcmp(argv[1],"pm-set-error") || !strcmp(argv[1],"pm-read-error") ||
        !strcmp(argv[1],"pm-read-mismatch") ||
        !strcmp(argv[1],"drop-during-retry"))));
    if(argc==2 && !strcmp(argv[1],"sdk-error"))sdk_result=-7;
    if(argc==2 && !strcmp(argv[1],"usb-snapshot"))usb_connected=true;
    if(argc==2 && !strcmp(argv[1],"drop-during-retry"))drop_during_retry=true;
    if(argc==2 && !strcmp(argv[1],"pm-set-error")){pm_result=-5;pm_read_value=0x00a11142u;}
    if(argc==2 && !strcmp(argv[1],"pm-read-error"))pm_read_result=-8;
    if(argc==2 && !strcmp(argv[1],"pm-read-mismatch"))pm_read_value=0x00a11142u;
    memset(test_flash_sector,0xff,sizeof test_flash_sector);
#if PM_TRACE_ENABLE
    for(unsigned i=0;i<64;i++)pm_trace_event(PM_TRACE_SLOW_LOOP,0,0,50000);
#endif
    if(!setjmp(finished))pm_firmware_entry();
    if(argc==2 && !strcmp(argv[1],"usb-snapshot")){
        puts("late USB snapshot begin");
        usb_connected=false;log_usb_snapshot();usb_connected=true;log_usb_snapshot();
        clock_ms+=14999;log_usb_snapshot();clock_ms++;log_usb_snapshot();
        puts("late USB snapshot end");
    }
    if(argc==2 && !strcmp(argv[1],"error-log")){
        state=ACTIVE;sta_on=wifi_enabled=true;ip_link=CYW43_LINK_UP;
        process(11,"{\"type\":\"error\",\"data\":{\"code\":\"dummy-private-token\"}}");
        assert(state==RETRY && sta_on && wifi_enabled && !strcmp(last_lcd_one,"SERVER"));
        state=ACTIVE;
        process(11,"{\"type\":\"error\",\"data\":{\"code\":\"stale_frame\"}}");
        state=ACTIVE;
        process(11,"{\"type\":\"error\",\"data\":{\"code\":\"invalid_ack_token\"}}");
        wifi_enabled=sta_on=false;pm_result=-5;unsigned calls=pm_calls;
        start_wifi();assert(state==WIFI && pm_calls==calls+1 && lwip_depth==0);
        pm_result=0;
    }
#if PM_TRACE_ENABLE
    if(!sdk_result)assert(pm_trace_dropped()>0);
    trace_output=true;
    for(unsigned i=0;i<100;i++)pm_trace_flush();
    if(!sdk_result)assert(strstr(trace_text,"event=button"));
    puts("app trace: full USB queue did not prevent button/network main-loop regression");
    if(!sdk_result)test_pm_diagnostics();
    test_frame_pipeline();
#endif
    puts("app timing: production main loop PASS");return 0;
}

#if PM_TRACE_STATS
void pm_net_rx_frame_start(uint64_t frame,uint64_t session){
    pm_rx_state s={0};pm_stats_frame_start();pm_rx_frame_begin(frame,session,&s);
}
void pm_net_rx_frame_end(pm_rx_reason reason,int error){
    pm_rx_state s={0};if(reason==PM_RX_COMPLETE)pm_stats_frame_done();pm_rx_finish(reason,error,&s);
}
void pm_net_rx_snapshot(void){}
#endif
