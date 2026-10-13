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

uint8_t test_flash_sector[4096];
cyw43_t cyw43_state;
static jmp_buf finished;
static uint32_t clock_ms=1000,confirm_loop_ms;
static int pressed=-1,wifi_link=CYW43_LINK_DOWN,ip_link=CYW43_LINK_DOWN;
static int sdk_result;
static unsigned joins,erases,server_begins,hello_count,heartbeat_count,loops;
static bool sta_on,server_ready,welcome_available;
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
void gpio_init(uint pin){assert(pin>=12 && pin<=15);}
void gpio_set_dir(uint pin,int dir){assert(pin>=12 && pin<=15 && dir==GPIO_IN);}
void gpio_pull_up(uint pin){assert(pin>=12 && pin<=15);}
bool gpio_get(uint pin){
    if(pin==PM_BUTTON_SUB_PIN && phase==CONFIRM_AWAY)confirm_loop_ms=clock_ms;
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
void pm_display_init(void){}
void pm_display_status(const char *a,const char *b){assert(a && b);clock_ms+=20;}
bool pm_display_frame(uint8_t *p,size_t n){assert(p && n==PM_IMAGE_BYTES);return true;}
void pm_display_standby(void){}
void pm_display_wake(void){}
void pm_display_reference_pattern(void){assert(!"normal build must not draw diagnostics");}
bool pm_net_init(void){return true;}
void pm_net_close(void){network_stage=PM_NET_DOWN;}
void pm_net_begin(const pm_profile *p){
    assert(p==&profiles[0] && !p->socks);server_begins++;network_stage=PM_NET_TCP;
}
void pm_net_poll(void){if(server_ready)network_stage=PM_NET_READY;}
pm_net_stage pm_net_state(void){return network_stage;}
bool pm_net_send(uint8_t kind,const char *j){
    assert(j && network_stage==PM_NET_READY);
    if(kind==1){hello_count++;welcome_available=true;}
    if(kind==5)heartbeat_count++;
    return true;
}
bool pm_net_take(uint8_t *kind,char *j,size_t n){
    if(!welcome_available)return false;
    const char welcome[]="{\"type\":\"welcome\",\"data\":{\"incarnation\":1,\"session_epoch\":1}}";
    assert(n>sizeof welcome);memcpy(j,welcome,sizeof welcome);*kind=2;
    welcome_available=false;return true;
}
void cyw43_arch_enable_sta_mode(void){sta_on=true;}
void cyw43_arch_disable_sta_mode(void){sta_on=false;wifi_link=ip_link=CYW43_LINK_DOWN;}
void cyw43_arch_poll(void){}
int cyw43_arch_wifi_connect_async(const char *ssid,const char *pw,uint32_t auth){
    assert(sta_on && wifi_enabled && auth==CYW43_AUTH_WPA2_AES_PSK);
    assert(ssid==profiles[active_profile].ssid && pw==profiles[active_profile].password);
    joins++;wifi_link=ip_link=CYW43_LINK_JOIN;return sdk_result;
}
int cyw43_wifi_link_status(cyw43_t *s,int i){assert(s==&cyw43_state && i==0);return wifi_link;}
int cyw43_tcpip_link_status(cyw43_t *s,int i){assert(s==&cyw43_state && i==0);return ip_link;}

static bool released(void){
    for(unsigned i=0;i<PM_BUTTON_COUNT;i++)if(buttons[i].raw || buttons[i].stable)return false;
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
        clock_ms=started_ms+settings.app_s*1000u;phase=SERVER_BOUNDARY;return;
    case SERVER_BOUNDARY:
        assert(state==SERVER);clock_ms++;phase=SERVER_TIMEOUT;return;
    case SERVER_TIMEOUT:
        assert(state==RETRY && !sta_on);
        puts("server timeout boundary: OK");clock_ms=started_ms+retry_ms;phase=HOME_RETRY;return;
    case HOME_RETRY:
        assert(state==WIFI && joins==6 && active_profile==0);
        ip_link=CYW43_LINK_UP;server_ready=true;phase=HOME_CONNECT;break;
    case HOME_CONNECT:
        if(state==ACTIVE){
            assert(hello_count==1 && heartbeat_count==0 && server_begins==2);
            puts("HOME recovery: Wi-Fi/IP, server, Hello/Welcome; no early heartbeat");
            standby();assert(state==STANDBY && !sta_on);phase=HOME_ACTIVE;
        }break;
    case HOME_ACTIVE:
        press_after_release(0);
        if(joins==7){
            assert(state==WIFI && active_profile==0);
            puts("standby wake: no immediate timeout");phase=WAKE;longjmp(finished,1);
        }break;
    case WAKE:assert(!"test already completed");
    }
    clock_ms+=ms;
}
int main(int argc,char **argv){
    assert(argc==1 || (argc==2 && !strcmp(argv[1],"sdk-error")));
    if(argc==2)sdk_result=-7;
    memset(test_flash_sector,0xff,sizeof test_flash_sector);
    if(!setjmp(finished))pm_firmware_entry();
    puts("app timing: production main loop PASS");return 0;
}
