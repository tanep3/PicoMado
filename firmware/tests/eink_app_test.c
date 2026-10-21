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
static unsigned draw_calls;
static uint8_t drawn_pixels[3813];
static uint64_t rendered_ack_frame;
uint64_t time_us_64(void){return (uint64_t)clock_ms*1000;}
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
void pm_display_feedback(pm_display_feedback_action a){(void)a;}
void pm_display_init(void){}
void pm_display_status(const char *a,const char *b){assert(a&&b);}
bool pm_display_frame(uint8_t *p,size_t n){assert(p&&n==3813&&!physical_busy);memcpy(drawn_pixels,p,n);draw_calls++;physical_busy=true;return true;}
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
    if(kind==9){render_count++;assert(number(j,"frame_id",&rendered_ack_frame));}
    if(kind==1){assert(strstr(j,"\"eink\""));assert(strstr(j,"eink-clock-v1"));assert(strstr(j,PM_BUTTON_COUNT==2?"buttons2":"buttons0"));}
    return true;
}
bool pm_net_send_optional(uint8_t kind,const char *j){return pm_net_send(kind,j);}
#if PM_EVENT_STREAM
bool pm_net_send_observed(uint8_t k,const char *j,uint64_t n,uint64_t p,uint64_t s){(void)n;(void)p;(void)s;return pm_net_send(k,j);}
void pm_net_observed_api(uint64_t operation,uint64_t frame){(void)operation;(void)frame;}
#endif
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

static pm_token next_frame(unsigned id){return (pm_token){incarnation,session_epoch,display_epoch+1,owner_epoch,id};}
static pm_api_result start_pending(pm_token t,uint8_t color,unsigned minutes,uint64_t minute,uint64_t width,bool bad_crc){
    static uint8_t pixels[3813];memset(pixels,color,sizeof pixels);
    uint32_t crc=pm_crc32(pixels,sizeof pixels);if(bad_crc)crc^=1;
    char policy[140]="";
    if(minutes)snprintf(policy,sizeof policy,",\"eink_refresh\":{\"full_refresh_minutes\":%u,\"minute\":%llu}",minutes,(unsigned long long)minute);
    snprintf(json,sizeof json,"{\"type\":\"frame_start\",\"data\":{\"token\":{\"device_id\":\"%s\",\"incarnation\":%llu,\"session_epoch\":%llu,\"display_epoch\":%llu,\"screen_owner_epoch\":%llu,\"frame_id\":%llu},\"mode\":\"normal\",\"width\":%llu,\"height\":122,\"format\":\"mono1\",\"byte_len\":3813,\"crc32\":%u%s}}",PM_DEVICE_ID,
        (unsigned long long)t.incarnation,(unsigned long long)t.session_epoch,(unsigned long long)t.display_epoch,
        (unsigned long long)t.screen_owner_epoch,(unsigned long long)t.frame_id,(unsigned long long)width,crc,policy);
    return api_frame(12,json);
}
static pm_api_result legacy_pending(pm_token t,uint8_t color,unsigned offset,unsigned count){
    int n=snprintf(json,sizeof json,"{\"type\":\"frame_chunk\",\"data\":{\"token\":{\"device_id\":\"%s\",\"incarnation\":%llu,\"session_epoch\":%llu,\"display_epoch\":%llu,\"screen_owner_epoch\":%llu,\"frame_id\":%llu},\"offset\":%u,\"bytes\":[",PM_DEVICE_ID,
        (unsigned long long)t.incarnation,(unsigned long long)t.session_epoch,(unsigned long long)t.display_epoch,
        (unsigned long long)t.screen_owner_epoch,(unsigned long long)t.frame_id,offset);
    for(unsigned i=0;i<count;i++)n+=snprintf(json+n,sizeof json-(size_t)n,"%s%u",i?",":"",color);
    assert(n>0&&(size_t)n+4<sizeof json);memcpy(json+n,"]}}",4);
    return api_frame(13,json);
}
static void pending_u64(uint8_t *p,uint64_t value){for(int n=7;n>=0;n--){p[n]=(uint8_t)value;value>>=8;}}
static void native_pending(pm_token t,uint8_t color){
    static uint8_t packet[1+sizeof(PM_DEVICE_ID)-1+44+3813];
    size_t id_length=strlen(PM_DEVICE_ID);packet[0]=(uint8_t)id_length;memcpy(packet+1,PM_DEVICE_ID,id_length);
    uint8_t *f=packet+1+id_length;
    pending_u64(f,t.incarnation);pending_u64(f+8,t.session_epoch);pending_u64(f+16,t.display_epoch);
    pending_u64(f+24,t.screen_owner_epoch);pending_u64(f+32,t.frame_id);memset(f+40,0,4);memset(f+44,color,3813);
    process_frame_bytes(packet,sizeof packet);
}
static void expect_pixels(uint8_t color){for(unsigned i=0;i<sizeof drawn_pixels;i++)assert(drawn_pixels[i]==color);}
static void finish_current(pm_token t){finish(true);assert(!drawing&&pending_render_ack&&pm_token_equal(pending_render_token,t));assert(send_pending_render_ack());assert(rendered_ack_frame==t.frame_id);}
static void pending_scenarios(void){
    unsigned before=render_count,calls=draw_calls;
    pm_token a=next_frame(100);assert(start_pending(a,0x11,0,0,250,false)==PM_API_PENDING);
    assert(legacy_pending(a,0x11,0,3813)==PM_API_PENDING);assert(drawing&&pm_token_equal(drawing_token,a));
    pm_token b=next_frame(101);assert(start_pending(b,0x22,20,500,250,false)==PM_API_PENDING);native_pending(b,0x22);
    assert(pending_frame.phase==PM_EINK_READY&&draw_calls==calls+1);expect_pixels(0x11);
    pm_token invalid=next_frame(102);sub_mode=true;
    assert(start_pending(invalid,0x33,10,501,(1ull<<32)+250,false)==PM_API_REJECTED);
    assert(start_pending(invalid,0x33,61,501,250,false)==PM_API_REJECTED);
    assert(pending_frame.phase==PM_EINK_READY&&pm_token_equal(pending_frame.frame.token,b)&&sub_mode);expect_pixels(0x11);
    finish(true);assert(drawing&&pm_token_equal(drawing_token,b)&&!pending_render_ack&&render_count==before);
    assert(draw_calls==calls+2&&clock_interval==20&&clock_minute==500&&clock_reset);expect_pixels(0x22);
    finish_current(b);poll_display();assert(draw_calls==calls+2&&render_count==before+1);
    // Repeated already-rendered token acknowledges without another physical draw.
    assert(start_pending(b,0x22,20,500,250,false)==PM_API_PENDING);
    assert(legacy_pending(b,0x22,0,3813)==PM_API_COMPLETED);assert(draw_calls==calls+2);
    assert(send_pending_render_ack());

    a=next_frame(200);assert(start_pending(a,0x41,0,0,250,false)==PM_API_PENDING);native_pending(a,0x41);
    b=next_frame(201);assert(start_pending(b,0x42,0,0,250,false)==PM_API_PENDING);
    assert(legacy_pending(b,0x42,0,100)==PM_API_PENDING);
    invalid=next_frame(202);assert(start_pending(invalid,0x43,0,0,251,false)==PM_API_REJECTED);
    assert(assembly.active&&assembly.used==100&&pending_frame.phase==PM_EINK_RECEIVING);
    calls=draw_calls;finish(true);assert(!drawing&&!pending_render_ack&&draw_calls==calls);
    pm_token c=next_frame(203);assert(start_pending(c,0x43,0,0,250,false)==PM_API_PENDING);
    assert(legacy_pending(c,0x43,0,3813)==PM_API_PENDING);assert(drawing&&pm_token_equal(drawing_token,c));expect_pixels(0x43);finish_current(c);

    a=next_frame(300);assert(start_pending(a,0x51,10,600,250,false)==PM_API_PENDING);native_pending(a,0x51);
    b=next_frame(301);assert(start_pending(b,0x52,15,601,250,false)==PM_API_PENDING);native_pending(b,0x52);
    c=next_frame(302);assert(start_pending(c,0x53,0,0,250,false)==PM_API_PENDING);native_pending(c,0x53);
    calls=draw_calls;unsigned clocks=clock_calls;finish(false);
    assert(drawing&&pm_token_equal(drawing_token,c)&&draw_calls==calls+1&&clock_calls==clocks);expect_pixels(0x53);finish_current(c);

    a=next_frame(400);assert(start_pending(a,0x61,0,0,250,false)==PM_API_PENDING);native_pending(a,0x61);
    b=next_frame(401);assert(start_pending(b,0x62,0,0,250,true)==PM_API_PENDING);
    assert(legacy_pending(b,0x62,0,3813)==PM_API_REJECTED&&pending_frame.phase==PM_EINK_EMPTY);
    calls=draw_calls;finish(true);assert(!drawing&&!pending_render_ack&&draw_calls==calls);
    c=next_frame(402);assert(start_pending(c,0x63,0,0,250,false)==PM_API_PENDING);native_pending(c,0x63);finish_current(c);

    for(unsigned invalidation=0;invalidation<4;invalidation++){
        a=next_frame(500+invalidation*2);assert(start_pending(a,0x71,0,0,250,false)==PM_API_PENDING);native_pending(a,0x71);
        b=next_frame(501+invalidation*2);assert(start_pending(b,0x72,10,700,250,false)==PM_API_PENDING);native_pending(b,0x72);
        assert(pending_frame.phase==PM_EINK_READY);before=render_count;calls=draw_calls;
        if(invalidation==0)local("NETWORK","HOME");
        else if(invalidation==1)retry();
        else if(invalidation==2){state=SESSION;assert(api_welcome(2,"{\"incarnation\":1,\"session_epoch\":77}")==PM_API_COMPLETED);}
        else standby();
        assert(pending_frame.phase==PM_EINK_EMPTY);finish(true);
        assert(!drawing&&draw_calls==calls&&render_count==before&&!pending_render_ack);
        if(invalidation==3)assert(state==STANDBY&&!panel_standby);
        state=ACTIVE;await_selection=false;wifi_enabled=true;have_display=false;
    }
    // Same token received before physical completion must coalesce at dispatch.
    a=next_frame(900);assert(start_pending(a,0x81,10,800,250,false)==PM_API_PENDING);native_pending(a,0x81);
    calls=draw_calls;before=render_count;
    assert(start_pending(a,0x81,10,800,250,false)==PM_API_PENDING);
    assert(legacy_pending(a,0x81,0,3813)==PM_API_PENDING&&pending_frame.phase==PM_EINK_READY);
    finish(true);assert(!drawing&&draw_calls==calls&&pending_frame.phase==PM_EINK_EMPTY);
    assert(pending_render_ack&&pm_token_equal(pending_render_token,a)&&render_count==before);
    assert(send_pending_render_ack()&&render_count==before+1);poll_display();assert(draw_calls==calls);
    // A failed first draw may retry the fully CRC-validated replay once.
    a=next_frame(901);assert(start_pending(a,0x82,0,0,250,false)==PM_API_PENDING);native_pending(a,0x82);
    assert(start_pending(a,0x82,0,0,250,false)==PM_API_PENDING);native_pending(a,0x82);
    calls=draw_calls;finish(false);assert(drawing&&draw_calls==calls+1&&!pending_render_ack);finish_current(a);
    // A replay of the active token cannot erase newer partial or Ready pixels.
    for(unsigned ready=0;ready<2;ready++){
        a=next_frame(910+ready*2);assert(start_pending(a,0x83,0,0,250,false)==PM_API_PENDING);native_pending(a,0x83);
        b=a;b.frame_id++;
        assert(start_pending(b,0x84,20,810+ready,250,false)==PM_API_PENDING);
        assert(legacy_pending(b,0x84,0,ready?3813:100)==PM_API_PENDING);
        size_t used=assembly.used;unsigned phase=pending_frame.phase;
        assert(start_pending(a,0x83,0,0,250,false)==PM_API_REJECTED);
        assert(legacy_pending(a,0x83,0,3813)==PM_API_REJECTED);
        assert(pending_frame.phase==phase&&pm_token_equal(pending_frame.frame.token,b)&&assembly.used==used);
        if(!ready)assert(legacy_pending(b,0x84,100,3713)==PM_API_PENDING);
        calls=draw_calls;finish(true);assert(drawing&&pm_token_equal(drawing_token,b)&&draw_calls==calls+1);
        expect_pixels(0x84);assert(clock_interval==20&&clock_minute==810+ready);finish_current(b);
    }
    puts("eink pending actual main: in-flight duplicate coalesces/success ACK/failed retry/newer partial+Ready preserved PASS");
    puts("eink pending actual main: busy latest/native+legacy/partial/CRC/invalid header/metadata/old failure/once/local/retry/reconnect/standby PASS");
}
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
    pending_scenarios();
    puts("eink app: negotiated clock metadata, interval changes, same-screen/reset routing, ordinary frame isolation, invalid policy rejection PASS");
    puts("eink app: production mono1 parse/CRC/Receipt, delayed Rendered, failed/stale completion, drain-before-standby, input capability PASS");
}
