#include <assert.h>
#include <stdio.h>
#include <string.h>
#define main pm_firmware_entry
#include "../src/main.c"
#undef main
uint8_t test_flash_sector[4096];
cyw43_t cyw43_state;
static uint32_t clock_ms=1000;
static unsigned receipt_count,render_count,joins,stops,panel_stops,clock_calls;
static bool clock_reset;static unsigned clock_interval;static uint64_t clock_minute;
static const char *frame_policy="";
static bool physical_busy,result_ready,result_ok;
absolute_time_t get_absolute_time(void){return clock_ms;}
uint32_t to_ms_since_boot(absolute_time_t t){return (uint32_t)t;}
void stdio_init_all(void){}
bool stdio_usb_connected(void){return false;}
void gpio_init(uint p){assert(PM_BUTTON_COUNT==2&&(p==20||p==21));}
void gpio_set_dir(uint p,int d){assert((p==20||p==21)&&d==GPIO_IN);}
void gpio_pull_up(uint p){assert(p==20||p==21);}
bool gpio_get(uint p){(void)p;return true;}
void sleep_ms(unsigned ms){clock_ms+=ms;}
void pico_get_unique_board_id_string(char *p,size_t n){assert(n>=17);memcpy(p,PM_EXPECTED_BOARD_ID,17);}
uint32_t save_and_disable_interrupts(void){return 0;}
void restore_interrupts(uint32_t n){(void)n;}
void flash_range_erase(uint32_t o,size_t n){(void)o;(void)n;}
void flash_range_program(uint32_t o,const uint8_t *p,size_t n){(void)o;(void)p;(void)n;}
const char *pm_display_capability(void){return "eink";}
void pm_display_init(void){}
void pm_display_status(const char *a,const char *b){assert(a&&b);}
bool pm_display_frame(uint8_t *p,size_t n){assert(p&&n==3813);physical_busy=true;return true;}
bool pm_display_clock_frame(uint8_t *p,size_t n,unsigned m,uint64_t minute,bool reset){clock_calls++;clock_interval=m;clock_minute=minute;clock_reset=reset;return pm_display_frame(p,n);}
void pm_display_standby(void){panel_stops++;}
void pm_display_wake(void){}
void pm_display_reference_pattern(void){assert(!"normal diagnostic");}
void pm_display_poll(void){}
bool pm_display_busy(void){return physical_busy;}
bool pm_display_take_result(bool *ok){if(!result_ready)return false;result_ready=false;*ok=result_ok;return true;}
bool pm_net_init(void){return true;}
void pm_net_close(void){stops++;}
void pm_net_begin(const pm_profile *p){(void)p;}
void pm_net_poll(void){}
pm_net_stage pm_net_state(void){return PM_NET_READY;}
pm_net_stage pm_net_checkpoint(void){return PM_NET_READY;}
bool pm_net_send(uint8_t kind,const char *j){
    assert(j);
    if(kind==8)receipt_count++;
    if(kind==9)render_count++;
    if(kind==1){assert(strstr(j,"\"eink\""));assert(strstr(j,"eink-clock-v1"));assert(strstr(j,PM_BUTTON_COUNT==2?"buttons2":"buttons0"));}
    return true;
}
bool pm_net_send_optional(uint8_t kind,const char *j){return pm_net_send(kind,j);}
size_t pm_net_payload_length(void){return 0;}
bool pm_net_take(uint8_t *k,char *j,size_t n){(void)k;(void)j;(void)n;return false;}
void cyw43_arch_enable_sta_mode(void){}
void cyw43_arch_disable_sta_mode(void){stops++;}
static unsigned lwip_depth;
void cyw43_arch_lwip_begin(void){assert(lwip_depth==0);lwip_depth=1;}
void cyw43_arch_lwip_end(void){assert(lwip_depth==1);lwip_depth=0;}
int cyw43_wifi_pm(cyw43_t *s,uint32_t value){assert(lwip_depth==1 && s==&cyw43_state && value==CYW43_NONE_PM);return 0;}
int cyw43_wifi_get_pm(cyw43_t *s,uint32_t *value){assert(lwip_depth==1 && s==&cyw43_state);*value=CYW43_NONE_PM;return 0;}
void cyw43_arch_poll(void){}
int cyw43_arch_wifi_connect_async(const char *s,const char *p,uint32_t a){assert(s&&p&&a==CYW43_AUTH_WPA2_AES_PSK);joins++;return 0;}
int cyw43_wifi_link_status(cyw43_t *s,int i){(void)s;(void)i;return CYW43_LINK_UP;}
int cyw43_tcpip_link_status(cyw43_t *s,int i){(void)s;(void)i;return CYW43_LINK_UP;}
static void frame(unsigned id){
    state=ACTIVE;await_selection=false;incarnation=session_epoch=display_epoch=1;
    static uint8_t pixels[3813];memset(pixels,0xff,sizeof pixels);
    char token[400];snprintf(token,sizeof token,"\"token\":{\"device_id\":\"%s\",\"incarnation\":1,\"session_epoch\":1,\"display_epoch\":1,\"screen_owner_epoch\":%llu,\"frame_id\":%u}",PM_DEVICE_ID,(unsigned long long)owner_epoch,id);
    snprintf(json,sizeof json,"{\"type\":\"frame_start\",\"data\":{%s,\"mode\":\"normal\",\"width\":250,\"height\":122,\"format\":\"mono1\",\"byte_len\":3813,\"crc32\":%u%s}}",token,pm_crc32(pixels,sizeof pixels),frame_policy);
    sub_mode=true;process(12,json);assert(assembly.active&&!sub_mode);
    int n=snprintf(json,sizeof json,"{\"type\":\"frame_chunk\",\"data\":{%s,\"offset\":0,\"bytes\":[",token);
    for(unsigned i=0;i<3813;i++){memcpy(json+n,"255",3);n+=3;if(i<3812)json[n++]=',';}
    memcpy(json+n,"]}}",4);process(13,json);
    assert(drawing&&physical_busy&&!pending_render_ack);
}
static void finish(bool ok){physical_busy=false;result_ready=true;result_ok=ok;poll_display();}
int main(void){
    defaults();wifi_enabled=true;assert(send_hello());
    frame(1);assert(receipt_count==1&&!render_count);poll_display();
    assert(drawing&&!pending_render_ack);finish(true);
    assert(!drawing&&pending_render_ack&&have_display);assert(send_pending_render_ack());assert(render_count==1);
    frame(2);finish(false);assert(!drawing&&!pending_render_ack&&state==ACTIVE);assert(render_count==1);
    frame(3);owner_epoch++;finish(true);assert(!pending_render_ack&&render_count==1);
    frame(4);standby();assert(panel_standby&&state==ACTIVE&&!wifi_enabled);
    finish(true);assert(state==STANDBY&&!panel_standby&&!pending_render_ack&&render_count==1);
    unsigned before=joins;
    if(PM_BUTTON_COUNT==2){button(1,false);assert(joins==before);button(2,true);assert(joins==before+1&&state==WIFI);}
    state=ACTIVE;have_display=false;
    frame_policy=",\"eink_refresh\":{\"full_refresh_minutes\":10,\"minute\":100}";
    frame(10);assert(clock_calls==1&&clock_interval==10&&clock_minute==100&&clock_reset);finish(true);send_pending_render_ack();
    frame_policy=",\"eink_refresh\":{\"full_refresh_minutes\":20,\"minute\":101}";
    frame(11);assert(clock_calls==2&&clock_interval==20&&clock_minute==101&&!clock_reset);finish(true);send_pending_render_ack();
    have_display=false;
    frame(12);assert(clock_calls==3&&clock_reset);finish(true);send_pending_render_ack();
    frame_policy="";frame(13);assert(clock_calls==3);finish(true);send_pending_render_ack();
    for(unsigned interval=0;interval<=61;interval+=61){
        char invalid[600];snprintf(invalid,sizeof invalid,"{\"type\":\"frame_start\",\"data\":{\"token\":{\"device_id\":\"%s\",\"incarnation\":1,\"session_epoch\":1,\"display_epoch\":1,\"screen_owner_epoch\":%llu,\"frame_id\":20},\"width\":250,\"height\":122,\"format\":\"mono1\",\"byte_len\":3813,\"crc32\":0,\"eink_refresh\":{\"full_refresh_minutes\":%u,\"minute\":102}}}",PM_DEVICE_ID,(unsigned long long)owner_epoch,interval);
        process(12,invalid);assert(!assembly.active&&clock_calls==3);
    }
    puts("eink app: negotiated clock metadata, interval changes, same-screen/reset routing, ordinary frame isolation, invalid policy rejection PASS");
    puts("eink app: production mono1 parse/CRC/Receipt, delayed Rendered, failed/stale completion, drain-before-standby, input capability PASS");
}
