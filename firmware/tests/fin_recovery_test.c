// Isolated test: include unmodified production net/main in one translation unit.
// Socket callbacks and hardware are deterministic host mocks, never real I/O.
#include <arpa/inet.h>
#include <assert.h>
#include <setjmp.h>
#include <stdio.h>
#include <string.h>
#ifndef PM_TEST_NET_SOURCE
#define PM_TEST_NET_SOURCE "../src/net.c"
#endif
#include PM_TEST_NET_SOURCE
#define main pm_firmware_entry
#ifndef PM_TEST_APP_MAIN_SOURCE
#define PM_TEST_APP_MAIN_SOURCE "../src/main.c"
#endif
#include PM_TEST_APP_MAIN_SOURCE
#undef main
#include "normal_usb_stubs.h"

uint8_t test_flash_sector[4096];
cyw43_t cyw43_state;
#if PM_TRACE_STATS
static struct stats_mem pool_stats,seg_stats;
struct stats_ lwip_stats={.memp={&pool_stats,&seg_stats}};
#endif
static uint64_t host_clock_us=1000000;
static unsigned lock_depth,physical_draws,tcp_closes,credits,status_updates;
static struct tcp_pcb sockets[2];
static unsigned socket_index;
static tcp_recv_fn callbacks[2];
static tcp_err_fn errors[2];
static unsigned test_case,loop_phase,sleeps;
static unsigned snd_space=65535,write_calls,output_calls;
static err_t write_result,output_result;
static bool error_on_write,error_on_output;
static uint64_t write_delay_us,output_delay_us;
static bool inject_fin_after_copy;
enum matrix_point { MP_NONE, MP_TOP_POLL, MP_COPY, MP_POST_POLL, MP_ACK_WRITE, MP_ACK_OUTPUT, MP_CLOSE_WRITE, MP_CLOSE_OUTPUT };
static struct {
    unsigned event,point,old_pixels,error,poll_target,user_state,user_button;
    uint64_t elapsed,spi_us;
    bool armed,injected,link_down,stop_sleep,display_fail;
} matrix;
static bool matrix_running;
static unsigned matrix_polls,matrix_tx_kind,matrix_expired_draws,matrix_start_draws,matrix_owner;
static unsigned matrix_expected_draws,matrix_expected_keep,matrix_expected_status;
static void matrix_prepare(void);
static void matrix_inject(void);
static jmp_buf done;
static uint8_t input[32768],tail_wire[5000],pixels[PM_IMAGE_BYTES];
static size_t input_length,tail_length;
static pm_token original_token;
static uint64_t fixture_started_us;

uint64_t time_us_64(void){return host_clock_us;}
absolute_time_t get_absolute_time(void){return host_clock_us/1000;}
uint32_t to_ms_since_boot(absolute_time_t t){return (uint32_t)t;}
void stdio_init_all(void){}
bool stdio_usb_connected(void){return false;}
size_t pm_trace_usb_write(const char *p,size_t n){(void)p;(void)n;return 0;}
void gpio_init(uint p){(void)p;}
void gpio_set_dir(uint p,int d){(void)p;(void)d;}
void gpio_pull_up(uint p){(void)p;}
bool gpio_get(uint p){(void)p;return true;}
void pico_get_unique_board_id_string(char *out,size_t n){assert(n>=17);memcpy(out,PM_EXPECTED_BOARD_ID,17);}
uint32_t save_and_disable_interrupts(void){return 0;}
void restore_interrupts(uint32_t s){(void)s;}
void flash_range_erase(uint32_t p,size_t n){(void)p;assert(n<=sizeof test_flash_sector);memset(test_flash_sector,255,n);}
void flash_range_program(uint32_t p,const uint8_t *v,size_t n){(void)p;assert(n<=sizeof test_flash_sector);memcpy(test_flash_sector,v,n);}
const char *pm_display_capability(void){return PM_DISPLAY_CAPABILITY;}
void pm_display_feedback(pm_display_feedback_action a){(void)a;}
void pm_display_init(void){}
void pm_display_status(const char *a,const char *b){assert(a&&b);status_updates++;}
bool pm_display_frame(uint8_t *p,size_t n){
    assert(p&&n==PM_IMAGE_BYTES&&!lock_depth);
    if(matrix_running&&matrix.armed){
        if((stage==PM_NET_DRAINING||stage==PM_NET_DRAINED)&&host_clock_us-eof_started_us>=PM_NET_EOF_DRAIN_US)matrix_expired_draws++;
        host_clock_us+=matrix.spi_us;
    }
    physical_draws++;return !(matrix_running&&matrix.armed&&matrix.display_fail);
}
void pm_display_standby(void){}
void pm_display_wake(void){}
void pm_display_reference_pattern(void){assert(!"unexpected diagnostic screen");}
int cyw43_arch_init(void){return 0;}
void cyw43_arch_enable_sta_mode(void){}
void cyw43_arch_disable_sta_mode(void){}
int cyw43_arch_wifi_connect_async(const char *a,const char *b,uint32_t c){(void)a;(void)b;(void)c;return 0;}
int cyw43_wifi_link_status(cyw43_t *s,int i){(void)s;(void)i;return CYW43_LINK_UP;}
int cyw43_tcpip_link_status(cyw43_t *s,int i){(void)s;(void)i;return matrix_running&&matrix.armed&&matrix.link_down?CYW43_LINK_DOWN:CYW43_LINK_UP;}
int cyw43_wifi_pm(cyw43_t *s,uint32_t v){(void)s;assert(v==CYW43_NONE_PM);return 0;}
uint get_core_num(void){return 0;}
uint __get_current_exception(void){return 0;}
int cyw43_ioctl(cyw43_t *s,uint32_t command,size_t length,uint8_t *buffer,uint32_t interface){
    (void)s;(void)command;(void)length;(void)buffer;(void)interface;
    assert(!"unrequested radio GET in existing application fixture");return -1;
}
int cyw43_wifi_get_pm(cyw43_t *s,uint32_t *v){(void)s;*v=CYW43_NONE_PM;return 0;}
void cyw43_arch_lwip_begin(void){assert(lock_depth==0);lock_depth=1;}
void cyw43_arch_lwip_end(void){
    assert(lock_depth==1);
    bool handoff=inject_fin_after_copy && last_payload_length>0 &&
       (uint8_t)json[0]==strlen(PM_DEVICE_ID);
    lock_depth=0;
    // Serialized IRQ handoff after main releases the SDK lock, before it
    // processes the already-owned copy returned by pm_net_take.
    if(matrix_running&&matrix.armed&&!matrix.injected&&matrix.point==MP_COPY&&last_payload_length>0)matrix_inject();
    if(handoff){
        inject_fin_after_copy=false;
        lock_depth=1;
        if(test_case==14)error_cb(NULL,-14);else assert(recv_cb(NULL,pcb,NULL,ERR_OK)==ERR_OK);
        lock_depth=0;
    }
}
uint32_t get_rand_32(void){return 0x12345678;}
int ipaddr_aton(const char *s,ip_addr_t *a){return inet_pton(AF_INET,s,&a->addr)==1;}
static unsigned slot(struct tcp_pcb *p){assert(p==&sockets[0]||p==&sockets[1]);return (unsigned)(p-sockets);}
struct tcp_pcb *tcp_new_ip_type(int t){assert(t==IPADDR_TYPE_V4);socket_index^=1;return &sockets[socket_index];}
void tcp_arg(struct tcp_pcb *p,void *a){assert(lock_depth==1);(void)slot(p);assert(!a);}
void tcp_recv(struct tcp_pcb *p,tcp_recv_fn cb){assert(lock_depth==1);callbacks[slot(p)]=cb;}
void tcp_err(struct tcp_pcb *p,tcp_err_fn cb){assert(lock_depth==1);errors[slot(p)]=cb;}
err_t tcp_connect(struct tcp_pcb *p,const ip_addr_t *a,uint16_t port,tcp_connected_fn cb){assert(lock_depth==1);(void)slot(p);assert(a&&port==16153&&cb);return ERR_OK;}
unsigned tcp_sndbuf(struct tcp_pcb *p){assert(lock_depth==1);(void)slot(p);return snd_space;}
err_t tcp_write(struct tcp_pcb *p,const void *b,size_t n,int f){assert(lock_depth==1);(void)slot(p);assert(b&&n&&f==TCP_WRITE_FLAG_COPY);write_calls++;
    const uint8_t *bytes=b;matrix_tx_kind=256+(bytes[0]&15);
    if(bytes[0]==0x82){size_t h=(bytes[1]&127)==126?4:2;assert(n>=h+10);matrix_tx_kind=bytes[h+4+5]^bytes[h+(5&3)];}
    if(matrix_running&&matrix.armed&&!matrix.injected&&
       ((matrix.point==MP_CLOSE_WRITE&&(matrix_tx_kind==264||matrix_tx_kind==266)) || (matrix.point==MP_ACK_WRITE&&(matrix_tx_kind==8||matrix_tx_kind==9))))matrix_inject();
    host_clock_us+=write_delay_us;if(error_on_write)error_cb(NULL,-14);return write_result;}
err_t tcp_output(struct tcp_pcb *p){assert(lock_depth==1);(void)slot(p);output_calls++;
    if(matrix_running&&matrix.armed&&!matrix.injected&&
       ((matrix.point==MP_CLOSE_OUTPUT&&(matrix_tx_kind==264||matrix_tx_kind==266)) || (matrix.point==MP_ACK_OUTPUT&&(matrix_tx_kind==8||matrix_tx_kind==9))))matrix_inject();
    host_clock_us+=output_delay_us;if(error_on_output)error_cb(NULL,-14);return output_result;}
err_t tcp_close(struct tcp_pcb *p){assert(lock_depth==1);(void)slot(p);tcp_closes++;return ERR_OK;}
void tcp_abort(struct tcp_pcb *p){assert(lock_depth==1);(void)slot(p);}
void tcp_recved(struct tcp_pcb *p,u16_t n){assert(lock_depth==1);(void)slot(p);credits+=n;}
unsigned pbuf_copy_partial(const struct pbuf *p,void *d,unsigned n,unsigned off){assert(off+n<=p->tot_len);memcpy(d,p->payload+off,n);return n;}
unsigned char pbuf_free(struct pbuf *p){assert(p);return 1;}

static void feed_only(const uint8_t *data,size_t n){
    struct pbuf p={(unsigned)n,data};assert(pcb&&callbacks[slot(pcb)]);
    cyw43_arch_lwip_begin();assert(callbacks[slot(pcb)](NULL,pcb,&p,ERR_OK)==ERR_OK);cyw43_arch_lwip_end();
}
static void fin_only(void){
    size_t before_rx=rx_used,before_app=app_used,before_credit=rx_flow.uncredited;
    cyw43_arch_lwip_begin();assert(callbacks[slot(pcb)](NULL,pcb,NULL,ERR_OK)==ERR_OK);cyw43_arch_lwip_end();
    assert(rx_used==before_rx&&app_used==before_app&&rx_flow.uncredited==before_credit);
    if(test_case==2)printf("FIN latched with rx=%zu app=%zu credit=%zu; no bytes consumed by callback\n",before_rx,before_app,before_credit);
}
static size_t app_message(uint8_t *out,uint8_t kind,const uint8_t *payload,size_t n){
    uint32_t len=(uint32_t)n+2;
    for(unsigned i=0;i<4;i++)out[i]=(uint8_t)(len>>(24-8*i));
    out[4]=1;out[5]=kind;memcpy(out+6,payload,n);return n+6;
}
static size_t ws(uint8_t *out,const uint8_t *body,size_t n){
    assert(n<65536);out[0]=0x82;out[1]=126;out[2]=(uint8_t)(n>>8);out[3]=(uint8_t)n;memcpy(out+4,body,n);return n+4;
}
static size_t native_wire(uint8_t *out,pm_token t,unsigned offset,unsigned count){
    uint8_t payload[4200],body[4300];size_t k=0,idlen=strlen(PM_DEVICE_ID);
    payload[k++]=(uint8_t)idlen;memcpy(payload+k,PM_DEVICE_ID,idlen);k+=idlen;
    uint64_t f[]={t.incarnation,t.session_epoch,t.display_epoch,t.screen_owner_epoch,t.frame_id};
    for(unsigned i=0;i<5;i++)for(unsigned j=0;j<8;j++)payload[k++]=(uint8_t)(f[i]>>(56-8*j));
    for(unsigned j=0;j<4;j++)payload[k++]=(uint8_t)(offset>>(24-8*j));
    memcpy(payload+k,pixels+offset,count);k+=count;
    size_t n=app_message(body,18,payload,k);return ws(out,body,n);
}
static void stream_start_token(pm_token t){
    char start[1200];
    snprintf(start,sizeof start,"{\"type\":\"frame_start\",\"data\":{\"token\":{\"device_id\":\"%s\",\"incarnation\":%llu,\"session_epoch\":%llu,\"display_epoch\":%llu,\"screen_owner_epoch\":%llu,\"frame_id\":%llu},\"mode\":\"normal\",\"width\":%u,\"height\":%u,\"format\":\"%s\",\"byte_len\":%u,\"crc32\":%lu,\"stream\":{\"fps\":12,\"sequence\":1,\"generation\":37}}}",PM_DEVICE_ID,
      (unsigned long long)t.incarnation,(unsigned long long)t.session_epoch,(unsigned long long)t.display_epoch,(unsigned long long)t.screen_owner_epoch,(unsigned long long)t.frame_id,
      PM_W,PM_H,PM_PIXEL_FORMAT_NAME,PM_IMAGE_BYTES,(unsigned long)pm_crc32(pixels,sizeof pixels));
    process(12,start);assert(assembly.active&&receiving_stream);
}
static size_t stream_start_wire(uint8_t *out,pm_token t){
    char start[1200];
    snprintf(start,sizeof start,"{\"type\":\"frame_start\",\"data\":{\"token\":{\"device_id\":\"%s\",\"incarnation\":%llu,\"session_epoch\":%llu,\"display_epoch\":%llu,\"screen_owner_epoch\":%llu,\"frame_id\":%llu},\"mode\":\"normal\",\"width\":%u,\"height\":%u,\"format\":\"%s\",\"byte_len\":%u,\"crc32\":%lu,\"stream\":{\"fps\":12,\"sequence\":1,\"generation\":37}}}",PM_DEVICE_ID,
      (unsigned long long)t.incarnation,(unsigned long long)t.session_epoch,(unsigned long long)t.display_epoch,(unsigned long long)t.screen_owner_epoch,(unsigned long long)t.frame_id,
      PM_W,PM_H,PM_PIXEL_FORMAT_NAME,PM_IMAGE_BYTES,(unsigned long)pm_crc32(pixels,sizeof pixels));
    char *seq=strstr(start,"\"sequence\":1");assert(seq);seq[strlen("\"sequence\":")]='2';
    uint8_t body[1500];size_t n=app_message(body,12,(const uint8_t *)start,strlen(start));return ws(out,body,n);
}
static void seed(void){
    defaults();state=ACTIVE;wifi_enabled=true;active_profile=0;
    incarnation=1;session_epoch=1586;display_epoch=1;owner_epoch=42;
    retained_pixels=false;memset(buttons,0,sizeof buttons);
    have_display=pending_render_ack=await_selection=pending_standby=pending_sub_cancel=sub_mode=false;
    pm_reset(&assembly);pm_video_reset(&video);receiving_stream=false;last_video_status_us=0;
    pm_net_begin(&profiles[0]);stage=PM_NET_READY;
    pcb->rcv_wnd=pcb->rcv_ann_wnd=TCP_WND;
    original_token=(pm_token){1,1586,1,42,495797};
    memset(pixels,0,sizeof pixels);stream_start_token(original_token);
    for(unsigned off=0;off<PM_IMAGE_BYTES-4096;){
        unsigned n=PM_IMAGE_BYTES-4096-off;if(n>4096)n=4096;
        receive_chunk(original_token,off,pixels+off,n);off+=n;
    }
    assert(assembly.used==PM_IMAGE_BYTES-4096&&!video.ready);
    tail_length=native_wire(tail_wire,original_token,PM_IMAGE_BYTES-4096,4096);
    assert(tail_length==4096+1+strlen(PM_DEVICE_ID)+44+6+4); // Same LCD ID and native-wire size as production.
    for(size_t n=0;n<20433;){size_t count=20433-n;if(count>tail_length)count=tail_length;memcpy(input+n,tail_wire,count);n+=count;}
    input_length=20433;physical_draws=0;tcp_closes=0;credits=0;status_updates=0;fixture_started_us=host_clock_us;
}

void cyw43_arch_poll(void){
    if(loop_phase==0){
        loop_phase=1;seed();
        if(matrix_running){matrix_prepare();return;}
        if(test_case==32||test_case==33||test_case==35||test_case==36){
            receive_chunk(original_token,PM_IMAGE_BYTES-4096,pixels+PM_IMAGE_BYTES-4096,4096);poll_video();assert(physical_draws==1);
            status_updates=0;retry_with_image(true);
            if(test_case==32||test_case==33)host_clock_us=(uint64_t)(started_ms+retry_ms)*1000;
            else {
                start_server_now(&profiles[0]);stage=PM_NET_READY;state=SESSION;
                process(2,"{\"type\":\"welcome\",\"data\":{\"incarnation\":1,\"session_epoch\":1587}}");
                if(test_case==36){pm_token t={1,1587,1,owner_epoch,495877};stream_start_token(t);for(unsigned off=0;off<PM_IMAGE_BYTES;){unsigned n=PM_IMAGE_BYTES-off;if(n>4096)n=4096;receive_chunk(t,off,pixels+off,n);off+=n;}video.next_us=host_clock_us+2000000;}
                fin_only();
            }
        }else if((test_case>=18 && test_case<=31 && test_case!=19)||(test_case>=37&&test_case<=40)){
            if(test_case!=20 && test_case!=30){receive_chunk(original_token,PM_IMAGE_BYTES-4096,pixels+PM_IMAGE_BYTES-4096,4096);assert(video.ready);}
            video.next_us=host_clock_us+(test_case==25?2000000:100000);
            uint8_t close_ws[16]={0x88,0};size_t n=2;
            if(test_case==20)feed_only(tail_wire,tail_length);
            if(test_case==22){close_ws[1]=1;close_ws[2]=0;n=3;}
            if(test_case==26){const uint8_t bad[]={0x88,4,3,0xe8,0xc0,0xaf};memcpy(close_ws,bad,sizeof bad);n=sizeof bad;}
            if(test_case==27)close_ws[0]=0x08;
            if(test_case==28){close_ws[1]=2;close_ws[2]=3;close_ws[3]=0xea;n=4;}
            if(test_case==29){close_ws[1]=2;close_ws[2]=3;n=3;}
            if(test_case==31){const uint8_t utf8[]={0x88,8,3,0xe9,0xe7,0xb5,0x82,0xe4,0xba,0x86};memcpy(close_ws,utf8,sizeof utf8);n=sizeof utf8;}
            if(test_case==37)write_result=ERR_MEM;
            if(test_case==38)n=1;
            if(test_case==39)output_result=-14;
            feed_only(close_ws,n);
            if(test_case==23||test_case==24){uint8_t k;char text[5000];assert(!pm_net_take(&k,text,sizeof text));
                if(test_case==24){
                    assert(!rx_used&&!app_used);feed_only(tail_wire,tail_length);assert(!rx_used&&!app_used);
                    assert(!pm_net_send(5,"{}")&&stage==PM_NET_DRAINED);
                    pending_render_token=original_token;pending_render_ack=true;assert(send_pending_render_ack()&&!pending_render_ack&&stage==PM_NET_DRAINED);
                }
            }
            if(test_case!=21)fin_only();
            if(test_case==23){cyw43_arch_lwip_begin();error_cb(NULL,-14);cyw43_arch_lwip_end();}
            if(test_case==40){
                uint8_t k;char text[5000];assert(!pm_net_take(&k,text,sizeof text)&&stage==PM_NET_DRAINED);
                struct pbuf bad={1,close_ws};cyw43_arch_lwip_begin();assert(recv_cb(NULL,pcb,&bad,-14)==ERR_OK);cyw43_arch_lwip_end();
            }
        }else if(test_case==1||test_case==2||test_case==6||test_case==9||test_case==10||test_case==12||test_case==13||test_case==14||test_case==15||test_case==16){
            if(test_case==9||test_case==10||test_case==12){video.fps=1;video.next_us=host_clock_us+(test_case==10?2000000:1000000);}
            if(test_case==13){memmove(input+3,input,input_length);input[0]=0x89;input[1]=1;input[2]=0x55;input_length+=3;}
            if(test_case==16)input[4+6+1+strlen(PM_DEVICE_ID)+44]^=1;
            feed_only(input,input_length);
            assert(rx_used==input_length&&rx_flow.uncredited==input_length-(RX_CAP-TCP_WND));
            if(test_case==2||test_case==9||test_case==10||test_case==12||test_case==13||test_case==15||test_case==16)fin_only();
            if(test_case==15){cyw43_arch_lwip_begin();assert(recv_cb(NULL,pcb,NULL,-14)==ERR_OK);cyw43_arch_lwip_end();}
            if(test_case==6||test_case==14)inject_fin_after_copy=true;
        }else if(test_case==11){
            size_t n=tail_length;memcpy(input,tail_wire,n);pm_token fresh=original_token;fresh.frame_id++;
            n+=stream_start_wire(input+n,fresh);
            for(unsigned off=0;off<PM_IMAGE_BYTES;){unsigned count=PM_IMAGE_BYTES-off;if(count>4096)count=4096;n+=native_wire(input+n,fresh,off,count);off+=count;}
            assert(n<RX_CAP);video.next_us=host_clock_us+100000;feed_only(input,n);fin_only();
        }else if(test_case==3){
            // One WS body has a benign first app message and a whole final chunk.
            uint8_t body[5000],wire[5100],kind;char ignored[32];
            size_t n=app_message(body,17,(const uint8_t *)"{}",2);
            memcpy(body+n,tail_wire+4,tail_length-4);n+=tail_length-4;
            size_t len=ws(wire,body,n);feed_only(wire,len);
            assert(pm_net_take(&kind,ignored,sizeof ignored)&&kind==17);
            assert(rx_used==0&&app_used==tail_length-4);fin_only();
        }else if(test_case==4){
            uint8_t kind;char ignored[5000];feed_only(tail_wire,1000);
            assert(!pm_net_take(&kind,ignored,sizeof ignored));assert(app_used>0&&ws_remaining>0);
            fin_only();
        }else if(test_case==17){
            receive_chunk(original_token,PM_IMAGE_BYTES-4096,pixels+PM_IMAGE_BYTES-4096,4096);assert(video.ready);
            const uint8_t invalid_ws[]={0x81,0};feed_only(invalid_ws,sizeof invalid_ws);
        }else if(test_case==5){
            receive_chunk(original_token,PM_IMAGE_BYTES-4096,pixels+PM_IMAGE_BYTES-4096,4096);
            assert(video.ready);fin_only();
        }else if(test_case==7){
            feed_only(input,input_length);cyw43_arch_lwip_begin();error_cb(NULL,-14);cyw43_arch_lwip_end();
        }else assert(!"bad case");
        return;
    }
    if(state==RETRY)longjmp(done,1);
    if(matrix_running&&matrix.armed){
        matrix_polls++;
        if(!matrix.injected&&((matrix.point==MP_TOP_POLL&&matrix_polls==1)||
           (matrix.point==MP_POST_POLL&&matrix_polls==matrix.poll_target)))matrix_inject();
    }
}
void sleep_ms(unsigned ms){
    host_clock_us+=(uint64_t)ms*1000;sleeps++;
    if(matrix_running){
        if(matrix.stop_sleep||state==RETRY||state==MENU||state==STANDBY)longjmp(done,1);
        assert(sleeps<2000);return;
    }
    if(test_case==32||test_case==33){
        assert(state==SERVER&&retained_pixels&&status_updates==0&&physical_draws==1);
        if(test_case==32)host_clock_us=(uint64_t)(started_ms+settings.app_s*1000u+1)*1000;
        else {state=SESSION;stage=PM_NET_READY;cyw43_arch_lwip_begin();error_cb(NULL,-14);cyw43_arch_lwip_end();}
        return;
    }
    if((test_case>=18 && test_case<=31 && test_case!=19)||(test_case>=37&&test_case<=40)){
        assert(state==ACTIVE&&host_clock_us-fixture_started_us<=1501000);
        if(test_case==25&&sleeps%50==0){const uint8_t close_ws[]={0x88,0};feed_only(close_ws,2);fin_only();}
        return;
    }
    if(test_case==36){assert(state==ACTIVE&&physical_draws==1&&status_updates==0);return;}
    if(test_case==1){assert(state==ACTIVE&&stage==PM_NET_READY&&physical_draws==1);longjmp(done,1);}
    if(test_case==2||test_case==6){assert(state==ACTIVE&&stage!=PM_NET_FAILED&&physical_draws==1);return;}
    if(test_case==9||test_case==10||test_case==11||test_case==13||test_case==16||test_case==18){
        if(test_case==10 && sleeps%50==0){fin_only();}
        assert(state==ACTIVE&&host_clock_us-fixture_started_us<=1501000);return;}
    if(test_case==12){assert(state==ACTIVE&&video.ready&&physical_draws==0);standby();longjmp(done,1);}
    if(test_case==14 && state==RETRY){assert(physical_draws==0);longjmp(done,1);}
    assert(!"discard cases must continue into RETRY before sleep");
}

static void run_main_case(unsigned c,const char *name){
    test_case=c;loop_phase=sleeps=0;inject_fin_after_copy=false;
    memset(test_flash_sector,255,sizeof test_flash_sector);
    if(!setjmp(done))pm_firmware_entry();
    unsigned observed_state=(unsigned)state;
    assert(!lock_depth);
    if(c==1){assert(physical_draws==1&&state==ACTIVE);}
    else if(c==32||c==33||c==35||c==36){
        assert(state==RETRY&&physical_draws==1);
        assert(status_updates==(c==33?1u:0u));assert(retained_pixels==(c!=33));
        if(c==36)assert(host_clock_us-fixture_started_us>=1500000);
    }
    else if(c==12){assert(state==STANDBY&&owner_epoch==43&&physical_draws==0);state=ACTIVE;poll_video();assert(!video.active&&physical_draws==0);}
    else {
        unsigned expected=(c==4||c==7||c==10||c==14||c==15||c==16||c==17||c==22||c==23||c==25||c==26||c==27||c==28||c==29||c==30||c==38||c==39||c==40)?0:1;
        assert(physical_draws==expected&&state==RETRY&&owner_epoch==43);
        assert(!rx_used&&!app_used&&!ws_remaining&&!assembly.active);
        if(c==9)assert(host_clock_us-fixture_started_us>=1000000&&host_clock_us-fixture_started_us<1500000);
        if(c==25)assert(host_clock_us-fixture_started_us>=1500000&&host_clock_us-fixture_started_us<1600000);
        if(c==10)assert(host_clock_us-fixture_started_us>=1500000&&host_clock_us-fixture_started_us<1600000);
        if(c==11)assert(video.received==2&&video.dropped==1&&video.last_rendered.frame_id==original_token.frame_id+1);
        if(expected){assert(!video.ready);state=ACTIVE;poll_video();assert(!video.active&&physical_draws==1);}
    }
    printf("PASS %s draws=%u observed_loop_state=%u rx=%zu app=%zu owner=%llu\n",name,physical_draws,observed_state,rx_used,app_used,(unsigned long long)owner_epoch);
    pm_net_close();
}
static void reconnect_tokens(void){
    loop_phase=99;inject_fin_after_copy=false;seed();
    struct tcp_pcb *old=pcb;unsigned oldslot=slot(old);
    feed_only(input,input_length);fin_only();pm_net_poll();assert(stage!=PM_NET_FAILED&&rx_used==20433);pm_net_close();
    assert(callbacks[oldslot]==NULL&&errors[oldslot]==NULL);
    retry();assert(owner_epoch==43&&!assembly.active);state=ACTIVE;poll_video();assert(!video.active);
    pm_net_begin(&profiles[0]);stage=PM_NET_READY;
    assert(pcb!=old&&callbacks[slot(pcb)]==recv_cb&&errors[slot(pcb)]==error_cb);
    struct pbuf stale_buf={10,input};cyw43_arch_lwip_begin();
    assert(recv_cb(NULL,old,&stale_buf,ERR_OK)==ERR_OK&&rx_used==0);
    cyw43_arch_lwip_end();
    state=SESSION;process(2,"{\"type\":\"welcome\",\"data\":{\"incarnation\":1,\"session_epoch\":1587}}");
    assert(state==ACTIVE&&session_epoch==1587&&owner_epoch==43&&display_epoch==0);
    pm_token fresh={1,1587,1,43,495877};stream_start_token(fresh);
    size_t old_length=native_wire(input,original_token,0,4096);feed_only(input,old_length);
    uint8_t kind;assert(pm_net_take(&kind,json,sizeof json)&&kind==18);
    process_frame_bytes((const uint8_t *)json,pm_net_payload_length());assert(assembly.used==0&&!video.ready);
    // A delayed FrameStart from the old session cannot replace the new assembly.
    pm_token current=assembly.token;
    char stale[1200];snprintf(stale,sizeof stale,"{\"type\":\"frame_start\",\"data\":{\"token\":{\"device_id\":\"%s\",\"incarnation\":1,\"session_epoch\":1586,\"display_epoch\":1,\"screen_owner_epoch\":42,\"frame_id\":495797}}}",PM_DEVICE_ID);
    process(12,stale);assert(pm_token_equal(assembly.token,current)&&assembly.active);
    for(unsigned off=0;off<PM_IMAGE_BYTES;){
        unsigned n=PM_IMAGE_BYTES-off;if(n>4096)n=4096;
        size_t len=native_wire(input,fresh,off,n);feed_only(input,len);
        assert(pm_net_take(&kind,json,sizeof json)&&kind==18);process_frame_bytes((const uint8_t *)json,pm_net_payload_length());off+=n;
    }
    assert(video.ready&&video.token.session_epoch==1587&&video.token.screen_owner_epoch==43);
    poll_video();assert(physical_draws==1&&video.last_rendered.session_epoch==1587);
    puts("PASS reconnect detaches old callbacks, resets transport, rejects old session/owner, draws fresh complete frame");
    pm_net_close();
}

// Transport-only matrices avoid SPI and frame construction; cases37-40 above
// additionally run the real main loop and assert physical draw/discard behavior.
static void net_fixture(void){
    loop_phase=99;test_case=41;inject_fin_after_copy=false;
    snd_space=65535;write_result=output_result=ERR_OK;error_on_write=error_on_output=false;
    write_delay_us=output_delay_us=0;
    defaults();pm_net_begin(&profiles[0]);stage=PM_NET_READY;
    pcb->rcv_wnd=pcb->rcv_ann_wnd=TCP_WND;
    write_calls=output_calls=credits=0;host_clock_us++;
}
static void take_none(void){uint8_t k;char text[4];assert(!pm_net_take(&k,text,sizeof text));}
static void assert_drain_deadline(void){
    assert(stage==PM_NET_DRAINED&&ws_closed);
    uint64_t original=eof_started_us;
    fin_only();assert(eof_started_us==original);
    host_clock_us=original+PM_NET_EOF_DRAIN_US-1;pm_net_poll();assert(stage==PM_NET_DRAINED);
    host_clock_us++;pm_net_poll();assert(stage==PM_NET_FAILED&&last_failure==PM_NET_FAIL_EOF_TIMEOUT);
}
static size_t close_wire(uint8_t *out,unsigned len){
    assert(len==0||(len>=2&&len<=125));out[0]=0x88;out[1]=(uint8_t)len;
    if(len){out[2]=3;out[3]=0xe8;memset(out+4,'a',len-2);}return len+2;
}
static void close_headers(void){
    unsigned count=0;uint8_t wire[131];
    // All FIN/RSV, mask and 7-bit length combinations, with FIN before/after
    // parsing or absent. Payloads use valid normal status and ASCII reason.
    for(unsigned first=8;first<256;first+=16)for(unsigned second=0;second<256;second++)for(unsigned fin_order=0;fin_order<3;fin_order++){
        net_fixture();unsigned len=second&127;memset(wire,'a',sizeof wire);wire[0]=(uint8_t)first;wire[1]=(uint8_t)second;
        if(len>=2&&len<=125){wire[2]=3;wire[3]=0xe8;}
        feed_only(wire,2+(len<=125?len:0));if(fin_order==1)fin_only();take_none();
        bool accepted=first==0x88&&!(second&128)&&len<=125&&len!=1;
        assert(stage==(accepted?PM_NET_DRAINED:PM_NET_FAILED));
        if(fin_order==2)fin_only();
        if(accepted)assert_drain_deadline();
        count++;
    }
    for(unsigned first=8;first<256;first+=16){
        net_fixture();wire[0]=(uint8_t)first;feed_only(wire,1);take_none();assert(stage==PM_NET_READY);
        fin_only();take_none();assert(stage==PM_NET_FAILED);count++;
    }
    printf("PASS Close headers/prefixes %u combinations\n",count);pm_net_close();
}
static void close_splits(void){
    const unsigned lengths[]={0,2,125};unsigned count=0;uint8_t wire[127];
    for(unsigned l=0;l<3;l++){
        size_t n=close_wire(wire,lengths[l]);
        for(size_t cut=0;cut<=n;cut++){
            net_fixture();if(cut)feed_only(wire,cut);take_none();
            assert(stage==(cut==n?PM_NET_DRAINED:PM_NET_READY));
            if(cut<n){feed_only(wire+cut,n-cut);take_none();assert(stage==PM_NET_DRAINED);}
            assert_drain_deadline();count++;
            net_fixture();if(cut)feed_only(wire,cut);fin_only();take_none();
            assert(stage==((cut==0||cut==n)?PM_NET_DRAINED:PM_NET_FAILED));count++;
        }
    }
    // The Close marker is ordinary data while a binary body is incomplete.
    const uint8_t partial[][3]={{0x82,2,0x88},{0x82,0,0},{0x89,0,0}};
    const size_t sizes[]={3,1,1};
    for(unsigned i=0;i<3;i++){net_fixture();feed_only(partial[i],sizes[i]);fin_only();take_none();take_none();assert(stage==PM_NET_DRAINED&&!ws_closed);count++;}
    printf("PASS Close segmentation/EOF %u boundaries\n",count);pm_net_close();
}
static void recv_hard(unsigned kind){
    assert(pcb);uint8_t data=0;struct pbuf p={1,&data};cyw43_arch_lwip_begin();
    if(kind==1)error_cb(NULL,-14);
    else assert(recv_cb(NULL,pcb,kind==2?NULL:&p,-14)==ERR_OK);
    cyw43_arch_lwip_end();assert(stage==PM_NET_FAILED);
}
static void close_errors(void){
    const unsigned lengths[]={0,2,125},spaces[]={0,5,6,7,8,9,65535};
    const err_t results[]={ERR_OK,ERR_MEM,-14};unsigned count=0;uint8_t wire[127];
    for(unsigned l=0;l<3;l++)for(unsigned space=0;space<7;space++)for(unsigned w=0;w<3;w++)for(unsigned o=0;o<3;o++)for(unsigned order=0;order<3;order++)for(unsigned error=0;error<12;error++){
        net_fixture();size_t n=close_wire(wire,lengths[l]),reply_n=lengths[l]?8:6;
        snd_space=spaces[space];write_result=results[w];output_result=results[o];
        // Before Close, after validated Close, after FIN, and nested SDK error
        // callbacks during write/output. ERR_MEM must never revive hard failure.
        if(error>=1&&error<=3){recv_hard(error);take_none();count++;continue;}
        error_on_write=error==10;error_on_output=error==11;
        feed_only(wire,n);if(order==1)fin_only();take_none();
        bool wrote=snd_space>=reply_n,output=wrote&&write_result==ERR_OK&&!error_on_write;
        bool hard=(wrote&&(write_result==-14||error_on_write)) || (output&&(output_result==-14||error_on_output));
        assert(write_calls==(unsigned)wrote&&output_calls==(unsigned)output);
        assert(stage==(hard?PM_NET_FAILED:PM_NET_DRAINED));
        if(!hard){
            if(error>=4&&error<=6){recv_hard(error-3);hard=true;}
            else {
                if(order==2||error>=7)fin_only();
                if(error>=7&&error<=9){recv_hard(error-6);hard=true;}
            }
        }
        if(hard){take_none();assert(stage==PM_NET_FAILED);}
        else assert_drain_deadline();
        count++;
    }
    printf("PASS Close/FIN/sndbuf/write/output/hard-error %u combinations\n",count);pm_net_close();
}
static void receive_bounds(void){
    unsigned count=0;memset(input,0x55,sizeof input);input[0]=0x88;input[1]=0;
    const size_t sizes[]={0,1,RX_CAP-1,RX_CAP,RX_CAP+1};
    for(unsigned i=0;i<5;i++){
        net_fixture();feed_only(input,sizes[i]);
        if(sizes[i]>RX_CAP){assert(stage==PM_NET_FAILED&&rx_used==0);}
        else {fin_only();take_none();assert(stage==(sizes[i]==1?PM_NET_FAILED:PM_NET_DRAINED));}
        count++;
    }
    const size_t used[]={0,1,RX_CAP-2,RX_CAP-1,RX_CAP};
    for(unsigned i=0;i<5;i++){
        size_t room=RX_CAP-used[i],incoming[]={0,1,2,room,room+1};
        for(unsigned j=0;j<5;j++){
            net_fixture();feed_only(input,used[i]);feed_only(input,incoming[j]);
            bool overflow=incoming[j]>room;
            assert(stage==(overflow?PM_NET_FAILED:PM_NET_READY));
            assert(rx_used==used[i]+(overflow?0:incoming[j])&&rx_used<=RX_CAP);
            assert(rx_flow.uncredited<=TCP_WND&&credits+rx_flow.uncredited==rx_used);count++;
        }
    }
    // Once Close is validated, even capacity+1 late bytes are never copied;
    // the same callback carrying a hard error must still fail immediately.
    for(unsigned error=0;error<2;error++){
        net_fixture();feed_only(input,2);take_none();assert(stage==PM_NET_DRAINED);
        struct pbuf p={RX_CAP+1,input};cyw43_arch_lwip_begin();assert(recv_cb(NULL,pcb,&p,error?-14:ERR_OK)==ERR_OK);cyw43_arch_lwip_end();
        assert(stage==(error?PM_NET_FAILED:PM_NET_DRAINED)&&rx_used==0&&app_used==0);count++;
    }
    printf("PASS receive capacity/credit/post-Close %u boundaries\n",count);pm_net_close();
}
static void application_bounds(void){
    unsigned count=0;static uint8_t body[RX_CAP],wire[RX_CAP];static char output[APP_CAP+1];uint8_t kind;
    // App lengths below/at/above APP_CAP-4, plus minimum and output NUL space.
    const size_t lengths[]={0,1,2,APP_CAP-5,APP_CAP-4,APP_CAP-3};
    for(unsigned i=0;i<6;i++)for(unsigned cap_axis=0;cap_axis<3;cap_axis++){
        size_t n=lengths[i],body_n=n+4;memset(body,'a',body_n);
        for(unsigned k=0;k<4;k++)body[k]=(uint8_t)(n>>(24-8*k));
        body[4]=1;body[5]=18;
        size_t wire_n=ws(wire,body,body_n);net_fixture();feed_only(wire,wire_n);fin_only();
        size_t payload=n>=2?n-2:0,cap=cap_axis==0?0:cap_axis==1?payload:payload+1;
        bool valid=n>=2&&n<=APP_CAP-4&&cap>payload;
        memset(output,0x5a,sizeof output);bool ok=pm_net_take(&kind,output,cap);
        assert(ok==valid);if(valid){assert(kind==18&&pm_net_payload_length()==payload&&output[payload]==0);take_none();assert(stage==PM_NET_DRAINED);}
        else assert(stage==PM_NET_FAILED);
        assert(output[cap]==0x5a);count++;
    }
    // Largest accepted WS body (10-byte header fills RX_CAP exactly) carries
    // two independent valid app messages and does not require a larger buffer.
    const size_t chunks[]={16378,16380};size_t used=0;
    for(unsigned i=0;i<2;i++){size_t n=chunks[i];for(unsigned k=0;k<4;k++)body[used+k]=(uint8_t)((n-4)>>(24-8*k));body[used+4]=1;body[used+5]=18;memset(body+used+6,'b',n-6);used+=n;}
    assert(used==RX_CAP-10);wire[0]=0x82;wire[1]=127;memset(wire+2,0,8);wire[8]=(uint8_t)(used>>8);wire[9]=(uint8_t)used;memcpy(wire+10,body,used);
    net_fixture();feed_only(wire,RX_CAP);fin_only();
    for(unsigned i=0;i<2;i++){assert(pm_net_take(&kind,output,sizeof output)&&kind==18&&pm_net_payload_length()==chunks[i]-6);}
    take_none();assert(stage==PM_NET_DRAINED);count++;
    // Oversized body declarations and incomplete extended headers terminate
    // finitely at EOF without indexing beyond the buffered bytes.
    const size_t declarations[]={RX_CAP-10,RX_CAP-9,RX_CAP-1,RX_CAP,65535};
    for(unsigned i=0;i<5;i++){
        net_fixture();size_t n=declarations[i];wire[0]=0x82;wire[1]=126;wire[2]=(uint8_t)(n>>8);wire[3]=(uint8_t)n;feed_only(wire,4);fin_only();take_none();take_none();
        assert(stage==(n>RX_CAP-10?PM_NET_FAILED:PM_NET_DRAINED));count++;
    }
    printf("PASS app/workspace/JSON-NUL/WS capacity %u boundaries\n",count);pm_net_close();
}
static void close_status_utf8(void){
    unsigned count=0;uint8_t wire[127]={0x88,2};
    for(unsigned code=0;code<65536;code++){
        net_fixture();wire[2]=(uint8_t)(code>>8);wire[3]=(uint8_t)code;feed_only(wire,4);fin_only();take_none();
        assert(stage==((code==1000||code==1001)?PM_NET_DRAINED:PM_NET_FAILED));count++;
    }
    const struct{uint8_t text[5];unsigned n;bool valid;} utf8[]={
        {{0},1,true},{{0x7f},1,true},{{0xc2,0x80},2,true},{{0xdf,0xbf},2,true},
        {{0xe0,0xa0,0x80},3,true},{{0xed,0x9f,0xbf},3,true},{{0xee,0x80,0x80},3,true},
        {{0xf0,0x90,0x80,0x80},4,true},{{0xf4,0x8f,0xbf,0xbf},4,true},
        {{0x80},1,false},{{0xc0,0xaf},2,false},{{0xc1,0xbf},2,false},{{0xc2},1,false},
        {{0xe0,0x9f,0xbf},3,false},{{0xed,0xa0,0x80},3,false},{{0xe1,0x80},2,false},
        {{0xf0,0x8f,0xbf,0xbf},4,false},{{0xf4,0x90,0x80,0x80},4,false},{{0xf5,0x80,0x80,0x80},4,false},
        {{0xf1,0x80,0x80},3,false},{{0xc2,0x7f},2,false},{{0xe1,0x80,0xc0},3,false}
    };
    for(unsigned i=0;i<sizeof utf8/sizeof utf8[0];i++){
        net_fixture();wire[0]=0x88;wire[1]=(uint8_t)(utf8[i].n+2);wire[2]=3;wire[3]=0xe8;memcpy(wire+4,utf8[i].text,utf8[i].n);
        feed_only(wire,utf8[i].n+4);fin_only();take_none();assert(stage==(utf8[i].valid?PM_NET_DRAINED:PM_NET_FAILED));count++;
    }
    printf("PASS Close status/UTF-8 %u combinations\n",count);pm_net_close();
}
static void close_send_deadline(void){
    unsigned count=0;const uint64_t delays[]={0,PM_NET_EOF_DRAIN_US-1,PM_NET_EOF_DRAIN_US};
    const err_t results[]={ERR_OK,ERR_MEM};const uint8_t wire[]={0x88,0};
    for(unsigned fin=0;fin<2;fin++)for(unsigned w=0;w<3;w++)for(unsigned o=0;o<3;o++)for(unsigned r=0;r<2;r++){
        net_fixture();uint64_t first=host_clock_us;if(fin){fin_only();host_clock_us+=100;}
        else first=host_clock_us;
        write_delay_us=delays[w];output_delay_us=delays[o];write_result=results[r];
        feed_only(wire,sizeof wire);take_none();assert(stage==PM_NET_DRAINED&&eof_started_us==first);
        uint64_t elapsed=host_clock_us-first;pm_net_poll();
        if(elapsed>=PM_NET_EOF_DRAIN_US)assert(stage==PM_NET_FAILED&&last_failure==PM_NET_FAIL_EOF_TIMEOUT);
        else assert_drain_deadline();
        count++;
    }
    printf("PASS Close enqueue/output latency keeps first EOF deadline %u boundaries\n",count);pm_net_close();
}
// Run the actual entry/loop for each scenario. No test-side deadline poll is
// inserted after a delayed send; only production checkpoints may stop draws.
static void matrix_inject(void){
    matrix.injected=true;host_clock_us=eof_started_us+matrix.elapsed;
    bool owned=lock_depth!=0;if(!owned)cyw43_arch_lwip_begin();
    if(matrix.event==20)error_cb(NULL,-14);
    else if(matrix.event==21)assert(recv_cb(NULL,pcb,NULL,-14)==ERR_OK);
    else if(matrix.event==22){uint8_t b=0;struct pbuf p={1,&b};assert(recv_cb(NULL,pcb,&p,-14)==ERR_OK);}
    else if(matrix.event==23){struct pbuf p={RX_CAP+1,input};assert(recv_cb(NULL,pcb,&p,ERR_OK)==ERR_OK);}
    if(!owned)cyw43_arch_lwip_end();
    if(matrix.point==MP_ACK_WRITE||matrix.point==MP_CLOSE_WRITE)write_result=matrix.error==1?ERR_MEM:matrix.error==2?-14:ERR_OK;
    if(matrix.point==MP_ACK_OUTPUT||matrix.point==MP_CLOSE_OUTPUT)output_result=matrix.error==1?ERR_MEM:matrix.error==2?-14:ERR_OK;
}
static void matrix_prepare(void){
    if(matrix.old_pixels){
        receive_chunk(original_token,PM_IMAGE_BYTES-4096,pixels+PM_IMAGE_BYTES-4096,4096);poll_video();assert(physical_draws==1);
        retry_with_image(true);start_server_now(&profiles[0]);stage=PM_NET_READY;state=SESSION;
        process(2,"{\"type\":\"welcome\",\"data\":{\"incarnation\":1,\"session_epoch\":1587}}");
        original_token=(pm_token){1,1587,1,owner_epoch,495877};stream_start_token(original_token);
        for(unsigned off=0;off<PM_IMAGE_BYTES-4096;){unsigned n=PM_IMAGE_BYTES-4096-off;if(n>4096)n=4096;receive_chunk(original_token,off,pixels+off,n);off+=n;}
        tail_length=native_wire(tail_wire,original_token,PM_IMAGE_BYTES-4096,4096);
    }
    bool partial=matrix.event==3||matrix.event==4||matrix.event==10||matrix.event==27||
        (matrix.event>=20&&matrix.event<=23);
    bool no_image=matrix.event==7;
    if(!partial&&!no_image){receive_chunk(original_token,PM_IMAGE_BYTES-4096,pixels+PM_IMAGE_BYTES-4096,4096);assert(video.ready);}
    if(matrix.event==4||matrix.event==10||matrix.event==27){receiving_stream=false;pm_video_reset(&video);}
    video.next_us=0;uint8_t close_ws[]={0x88,0},ping_ws[]={0x89,0},bad_ws[]={0x88,1,0};
    if(matrix.event==0||matrix.event==1||matrix.event==9||matrix.event==26)feed_only(close_ws,2);
    else if(matrix.event==2)feed_only(ping_ws,2);
    else if(matrix.event==24)feed_only(bad_ws,3);
    else if(matrix.event==25)feed_only(close_ws,1);
    else if(partial)feed_only(tail_wire,tail_length);
    else if(matrix.event==5){
        const char position[]="{\"type\":\"plugin_position\",\"data\":{}}";uint8_t body[100],wire[110];
        size_t n=app_message(body,17,(const uint8_t *)position,strlen(position));n=ws(wire,body,n);
        for(unsigned i=0;i<8;i++)feed_only(wire,n);
        pending_render_token=original_token;pending_render_ack=true;
    }
    if(matrix.event!=0&&matrix.event!=7)fin_only();
    matrix_owner=(unsigned)owner_epoch;matrix_start_draws=physical_draws;status_updates=0;
    last_payload_length=0;matrix_polls=matrix_expired_draws=0;fixture_started_us=host_clock_us;matrix.armed=true;
    if(matrix.event==7){
        state=(app_state)matrix.user_state;
        if(matrix.elapsed){fin_only();host_clock_us=eof_started_us+PM_NET_EOF_DRAIN_US;}
        button(matrix.user_button,false);
    }
    // Wi-Fi loss and SPI cases set their time before the first main checkpoint.
    if(matrix.event==8||matrix.event==9||matrix.event==10||matrix.event==26||matrix.event==27){
        host_clock_us=eof_started_us+matrix.elapsed;matrix.injected=true;
    }
}
static void matrix_run(void){
    matrix.armed=matrix.injected=false;matrix_running=true;matrix_polls=matrix_expired_draws=0;
    loop_phase=sleeps=0;test_case=48;inject_fin_after_copy=false;
    snd_space=65535;write_result=output_result=ERR_OK;write_delay_us=output_delay_us=0;
    error_on_write=error_on_output=false;retained_pixels=false;memset(test_flash_sector,255,sizeof test_flash_sector);
    if(!setjmp(done))pm_firmware_entry();
    if(matrix.event==7){
        bool keep=matrix.user_state!=ACTIVE&&(matrix.user_button==1||matrix.user_button==3);
        assert(retained_pixels==keep&&physical_draws==1&&matrix_expired_draws==0);
        if(matrix.user_state!=ACTIVE&&matrix.user_button==0)assert(state==MENU);
        if(matrix.user_state!=ACTIVE&&matrix.user_button==2)assert(state==STANDBY);
    }else{
        if(physical_draws!=matrix_start_draws+matrix_expected_draws || retained_pixels!=matrix_expected_keep || status_updates!=matrix_expected_status || matrix_expired_draws){
            fprintf(stderr,"matrix event=%u point=%u elapsed=%llu old=%u err=%u draws=%u/%u keep=%u/%u status=%u/%u late=%u state=%u\n",matrix.event,matrix.point,(unsigned long long)matrix.elapsed,matrix.old_pixels,matrix.error,physical_draws,matrix_start_draws+matrix_expected_draws,retained_pixels,matrix_expected_keep,status_updates,matrix_expected_status,matrix_expired_draws,(unsigned)state);
            assert(!"main recovery invariant");
        }
        assert(state==RETRY&&owner_epoch==matrix_owner+1&&!assembly.active&&!rx_used&&!app_used&&!ws_remaining);
        // Owner invalidation must prevent the abandoned ready pixels being
        // rendered even if a caller incorrectly tries video again after retry.
        unsigned draws=physical_draws;state=ACTIVE;poll_video();assert(physical_draws==draws&&!video.active);state=RETRY;
    }
    matrix.armed=false;matrix_running=false;pm_net_close();
}
static void matrix_expect(bool draw,bool hard){
    matrix_expected_draws=draw;matrix_expected_keep=!hard&&(matrix.old_pixels||draw);
    matrix_expected_status=!matrix_expected_keep;
}
static void main_close_matrix(void){
    unsigned count=0;const uint64_t delays[]={PM_NET_EOF_DRAIN_US-1,PM_NET_EOF_DRAIN_US,PM_NET_EOF_DRAIN_US+1};
    for(unsigned old=0;old<2;old++)for(unsigned event=0;event<3;event++)for(unsigned output=0;output<2;output++)for(unsigned d=0;d<3;d++)for(unsigned error=0;error<(event==2?1u:3u);error++){
        memset(&matrix,0,sizeof matrix);matrix.old_pixels=old;matrix.event=event;matrix.point=output?MP_CLOSE_OUTPUT:MP_CLOSE_WRITE;matrix.elapsed=delays[d];matrix.error=error;
        bool hard=error==2;matrix_expect(d==0&&!hard,hard);matrix_run();assert(matrix.injected);count++;
    }
    printf("PASS actual-main Close/pong write/output/FIN deadlines %u scenarios\n",count);
}
static void main_processing_matrix(void){
    unsigned count=0;const unsigned points[]={MP_TOP_POLL,MP_COPY,MP_POST_POLL,MP_ACK_WRITE,MP_ACK_OUTPUT};
    const uint64_t delays[]={PM_NET_EOF_DRAIN_US-1,PM_NET_EOF_DRAIN_US,PM_NET_EOF_DRAIN_US+1};
    for(unsigned old=0;old<2;old++)for(unsigned event=3;event<=5;event++)for(unsigned p=0;p<5;p++)for(unsigned d=0;d<3;d++){
        if(event==3&&p>=3)continue;
        memset(&matrix,0,sizeof matrix);matrix.old_pixels=old;matrix.event=event;matrix.point=points[p];matrix.poll_target=2;matrix.elapsed=delays[d];
        bool after_static_draw=event==4&&points[p]==MP_POST_POLL;
        matrix_expect(d==0||after_static_draw,false);matrix_run();assert(matrix.injected);count++;
    }
    // The last per-message poll can also expire exactly at the receive budget.
    for(unsigned old=0;old<2;old++){
        memset(&matrix,0,sizeof matrix);matrix.old_pixels=old;matrix.event=5;matrix.point=MP_POST_POLL;matrix.poll_target=9;matrix.elapsed=PM_NET_EOF_DRAIN_US;
        matrix_expect(false,false);matrix_run();assert(matrix.injected);count++;
    }
    printf("PASS actual-main poll/take/process/static-receipt/render-ack %u scenarios\n",count);
}
static void main_user_matrix(void){
    const unsigned states[]={ACTIVE,WIFI,SERVER,SESSION,RETRY,ENDPOINT};unsigned count=0;
    for(unsigned s=0;s<6;s++)for(unsigned b=0;b<4;b++)for(unsigned eof=0;eof<2;eof++){
        if(eof&&states[s]!=ACTIVE&&states[s]!=SERVER&&states[s]!=SESSION)continue;
        memset(&matrix,0,sizeof matrix);matrix.old_pixels=1;matrix.event=7;matrix.user_state=states[s];matrix.user_button=b;matrix.elapsed=eof;matrix.stop_sleep=true;
        matrix_run();count++;
    }
    printf("PASS actual-main user actions/state/EOF retention %u scenarios\n",count);
}
static void main_error_matrix(void){
    unsigned count=0;const uint64_t delays[]={0,PM_NET_EOF_DRAIN_US,PM_NET_EOF_DRAIN_US+1};
    for(unsigned old=0;old<2;old++)for(unsigned event=20;event<=25;event++)for(unsigned copy=0;copy<2;copy++)for(unsigned d=0;d<3;d++){
        if(copy&&event>=24)continue;
        memset(&matrix,0,sizeof matrix);matrix.old_pixels=old;matrix.event=event;matrix.point=copy?MP_COPY:MP_TOP_POLL;matrix.elapsed=delays[d];
        bool hard=event<=23||d==0;matrix_expect(false,hard);matrix_run();assert(matrix.injected);count++;
    }
    for(unsigned old=0;old<2;old++)for(unsigned expired=0;expired<2;expired++){
        memset(&matrix,0,sizeof matrix);matrix.old_pixels=old;matrix.event=8;matrix.link_down=true;matrix.elapsed=expired?PM_NET_EOF_DRAIN_US:0;
        matrix_expect(false,false);matrix_run();count++;
    }
    printf("PASS actual-main hard-error/overflow/protocol/expiry/Wi-Fi loss %u scenarios\n",count);
}
static void main_spi_matrix(void){
    unsigned count=0;
    for(unsigned old=0;old<2;old++)for(unsigned event=9;event<=10;event++)for(unsigned start=0;start<2;start++)for(unsigned over=0;over<2;over++){
        memset(&matrix,0,sizeof matrix);matrix.old_pixels=old;matrix.event=event;matrix.elapsed=start?PM_NET_EOF_DRAIN_US-1:0;matrix.spi_us=PM_NET_EOF_DRAIN_US+over;
        matrix_expect(true,false);matrix_run();count++;
    }
    for(unsigned old=0;old<2;old++)for(unsigned event=26;event<=27;event++){
        memset(&matrix,0,sizeof matrix);matrix.old_pixels=old;matrix.event=event;matrix.display_fail=true;
        matrix_expect(true,true);matrix_run();count++;
    }
    printf("PASS actual-main already-started SPI completion/display failure %u scenarios\n",count);
}

int main(int argc,char **argv){
    assert(argc==2);unsigned c=(unsigned)atoi(argv[1]);
    switch(c){
    case 1:run_main_case(1,"no-FIN-control");break;
    case 2:run_main_case(2,"pending20433-FIN-preserves-final-image");break;
    case 3:run_main_case(3,"whole-app-message-FIN");break;
    case 4:run_main_case(4,"partial-WS-never-displayed");break;
    case 5:run_main_case(5,"ready-frame-FIN");break;
    case 6:run_main_case(6,"FIN-after-main-copy");break;
    case 7:run_main_case(7,"hard-error-aborts");break;
    case 8:reconnect_tokens();break;
    case 9:run_main_case(9,"EOF-one-fps-pacing");break;
    case 10:run_main_case(10,"EOF-absolute-1500ms-limit");break;
    case 11:run_main_case(11,"EOF-completed-latest-replaces-ready");break;
    case 12:run_main_case(12,"EOF-standby-invalidates-owner");break;
    case 13:run_main_case(13,"EOF-ping-does-not-end-drain-early");break;
    case 14:run_main_case(14,"hard-error-after-main-copy-never-draws");break;
    case 15:run_main_case(15,"hard-error-after-EOF-aborts");break;
    case 16:run_main_case(16,"CRC-invalid-complete-at-EOF-never-draws");break;
    case 17:run_main_case(17,"decoder-failure-never-salvages-ready-frame");break;
    case 18:run_main_case(18,"valid-WS-Close-FIN-keeps-ready-until-pacing");break;
    case 19:{
        loop_phase=99;test_case=19;seed();receive_chunk(original_token,PM_IMAGE_BYTES-4096,pixels+PM_IMAGE_BYTES-4096,4096);poll_video();assert(physical_draws==1&&have_display);
        status_updates=0;retry_with_image(true);assert(status_updates==0&&state==RETRY);
        start_server_now(&profiles[0]);assert(status_updates==0&&state==SERVER);
        stage=PM_NET_READY;state=SESSION;process(2,"{\"type\":\"welcome\",\"data\":{\"incarnation\":1,\"session_epoch\":1587}}");
        assert(state==ACTIVE&&status_updates==0&&retained_pixels);
        pm_token fresh={1,1587,1,owner_epoch,495877};stream_start_token(fresh);
        for(unsigned off=0;off<PM_IMAGE_BYTES;){unsigned n=PM_IMAGE_BYTES-off;if(n>4096)n=4096;receive_chunk(fresh,off,pixels+off,n);off+=n;}
        poll_video();assert(physical_draws==2&&!retained_pixels&&status_updates==0);
        start_server_now(&profiles[0]);assert(status_updates==1);
        puts("PASS TR2 retains through reconnect/welcome until valid fresh image");break;}

    case 20:run_main_case(20,"last-chunk-then-Close-FIN");break;
    case 21:run_main_case(21,"normal-Close-without-FIN");break;
    case 22:run_main_case(22,"invalid-one-byte-Close");break;
    case 23:run_main_case(23,"hard-error-after-validated-Close");break;
    case 24:run_main_case(24,"payload-after-Close-never-accepted");break;
    case 25:run_main_case(25,"duplicate-Close-FIN-cannot-extend-1500ms");break;
    case 26:run_main_case(26,"invalid-Close-UTF8");break;
    case 27:run_main_case(27,"fragmented-Close-rejected");break;
    case 28:run_main_case(28,"error-Close-code-not-salvaged");break;
    case 29:run_main_case(29,"truncated-Close-FIN-rejected");break;
    case 30:run_main_case(30,"partial-image-Close-not-displayed");break;
    case 31:run_main_case(31,"going-away-Close-valid-Japanese-UTF8");break;
    case 32:run_main_case(32,"retention-through-main-retry-and-handshake-timeout");break;
    case 33:run_main_case(33,"hard-handshake-error-clears-retention");break;
    case 34:{
        loop_phase=99;test_case=34;seed();receive_chunk(original_token,PM_IMAGE_BYTES-4096,pixels+PM_IMAGE_BYTES-4096,4096);poll_video();status_updates=0;retry_with_image(true);start_server_now(&profiles[0]);
        button(1,false);assert(retained_pixels&&status_updates==0);button(0,false);assert(state==MENU&&!retained_pixels&&status_updates==1);
        start_server_now(&profiles[0]);assert(status_updates==2);retry_with_image(true);standby();assert(!retained_pixels&&state==STANDBY);
        puts("PASS explicit menu/standby clear retention, ignored setup button does not");break;}
    case 35:run_main_case(35,"normal-EOF-before-new-image-keeps-retained-pixels");break;
    case 36:run_main_case(36,"finite-EOF-timeout-keeps-previous-valid-pixels");break;
    case 37:run_main_case(37,"optional-Close-positive-sndbuf-ERR_MEM-keeps-ready");break;
    case 38:run_main_case(38,"one-octet-Close-FIN-rejected");break;
    case 39:run_main_case(39,"Close-output-hard-error-never-draws");break;
    case 40:run_main_case(40,"Close-followed-by-data-bearing-hard-error-never-draws");break;
    case 41:close_headers();break;
    case 42:close_splits();break;
    case 43:close_errors();break;
    case 44:receive_bounds();break;
    case 45:application_bounds();break;
    case 46:close_status_utf8();break;
    case 47:close_send_deadline();break;
    case 48:main_close_matrix();break;
    case 49:main_processing_matrix();break;
    case 50:main_user_matrix();break;
    case 51:main_error_matrix();break;
    case 52:main_spi_matrix();break;
    default:assert(!"invalid case");
    }
    return 0;
}
