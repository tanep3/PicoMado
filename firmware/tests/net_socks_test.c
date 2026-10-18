// Feed split proxy/HTTP replies through production net.c and its receive callback.
#include <arpa/inet.h>
#include <assert.h>
#include <stdio.h>
#include <string.h>
#ifndef PM_NET_SOURCE
#define PM_NET_SOURCE "../src/net.c"
#endif
#include PM_NET_SOURCE
cyw43_t cyw43_state;
#if PM_TRACE_STATS
static struct stats_mem pool_stats,seg_stats;
struct stats_ lwip_stats={.memp={&pool_stats,&seg_stats}};
#endif
static struct tcp_pcb socket_instance;
static tcp_connected_fn connect_callback;
static tcp_recv_fn receive_callback;
static uint8_t sent[4096];
static size_t sent_length;
static unsigned sends;
static unsigned credits_returned;
static int write_result,output_result;
#if PM_TRACE_ENABLE
static uint64_t clock_us;static char trace_text[32768];static size_t trace_used;
uint64_t time_us_64(void){return clock_us;}
size_t pm_trace_usb_write(const char *p,size_t n){
    assert(trace_used+n<sizeof trace_text);memcpy(trace_text+trace_used,p,n);trace_used+=n;trace_text[trace_used]=0;return n;
}
#endif
static unsigned send_room=sizeof sent;
static const pm_profile test_profile={"dummy-ssid","dummy-password","198.51.100.1",true,
    "192.0.2.1","dummy-user","dummy-proxy-password",1080};
int cyw43_arch_init(void){return 0;}
static unsigned lwip_depth;
void cyw43_arch_lwip_begin(void){assert(lwip_depth==0);lwip_depth=1;}
void cyw43_arch_lwip_end(void){assert(lwip_depth==1);lwip_depth=0;}
void cyw43_arch_poll(void){}
uint32_t save_and_disable_interrupts(void){return 0;}
void restore_interrupts(uint32_t state){(void)state;}
uint32_t get_rand_32(void){return 0x12345678;}
int ipaddr_aton(const char *text,ip_addr_t *address){return inet_pton(AF_INET,text,&address->addr)==1;}
struct tcp_pcb *tcp_new_ip_type(int t){assert(t==IPADDR_TYPE_V4);return &socket_instance;}
void tcp_arg(struct tcp_pcb *p,void *arg){assert(lwip_depth==1);assert(p==&socket_instance && !arg);}
void tcp_recv(struct tcp_pcb *p,tcp_recv_fn cb){assert(lwip_depth==1);assert(p==&socket_instance);receive_callback=cb;}
void tcp_err(struct tcp_pcb *p,tcp_err_fn cb){assert(lwip_depth==1);assert(p==&socket_instance);(void)cb;}
err_t tcp_connect(struct tcp_pcb *p,const ip_addr_t *a,uint16_t port,tcp_connected_fn cb){
    assert(lwip_depth==1);assert(p==&socket_instance && a && port==1080);connect_callback=cb;return ERR_OK;
}
unsigned tcp_sndbuf(struct tcp_pcb *p){assert(lwip_depth==1);assert(p==&socket_instance);return send_room;}
err_t tcp_write(struct tcp_pcb *p,const void *bytes,size_t n,int flags){
    assert(lwip_depth==1);assert(p==&socket_instance && n<=sizeof sent && flags==TCP_WRITE_FLAG_COPY);
    if(write_result)return write_result;
    memcpy(sent,bytes,n);sent_length=n;sends++;return ERR_OK;
}
err_t tcp_output(struct tcp_pcb *p){assert(lwip_depth==1);assert(p==&socket_instance);
#if PM_TRACE_ENABLE
    clock_us+=60000;
#endif
    return output_result;
}
err_t tcp_close(struct tcp_pcb *p){assert(lwip_depth==1);assert(p==&socket_instance);return ERR_OK;}
void tcp_abort(struct tcp_pcb *p){assert(lwip_depth==1);assert(p==&socket_instance);}
void tcp_recved(struct tcp_pcb *p,u16_t n){assert(lwip_depth==1);assert(p==&socket_instance);credits_returned+=n;}
unsigned pbuf_copy_partial(const struct pbuf *p,void *out,unsigned n,unsigned offset){
    assert(offset+n<=p->tot_len);memcpy(out,p->payload+offset,n);return n;
}
unsigned char pbuf_free(struct pbuf *p){assert(p);return 1;}
static void feed(const uint8_t *data,size_t n){
    struct pbuf buffer={(unsigned)n,data};assert(receive_callback);
    cyw43_arch_lwip_begin();
    assert(receive_callback(NULL,&socket_instance,&buffer,ERR_OK)==ERR_OK);
    cyw43_arch_lwip_end();pm_net_poll();
}
static void begin(void){
    pm_net_begin(&test_profile);assert(stage==PM_NET_TCP);
    cyw43_arch_lwip_begin();
    assert(connect_callback(NULL,&socket_instance,ERR_OK)==ERR_OK);
    cyw43_arch_lwip_end();
    const uint8_t expected[]={5,1,2};
    assert(stage==PM_NET_SOCKS && sent_length==3 && !memcmp(sent,expected,3));
}
static void authenticated(void){
    begin();unsigned before=sends;
    const uint8_t method[]={5,2};feed(method,1);assert(sends==before && stage==PM_NET_SOCKS);
    feed(method+1,1);assert(sent[0]==1 && stage==PM_NET_SOCKS);
    // Credentials were copied to TCP and cleared from the original workspace.
    for(size_t i=0;i<sizeof socks_auth_request;i++)assert(socks_auth_request[i]==0);
    before=sends;const uint8_t auth[]={1,0};feed(auth,1);assert(sends==before);
    feed(auth+1,1);
    assert(stage==PM_NET_SOCKS && sends==before+1 && sent_length==10);
    const uint8_t expected[]={5,1,0,1,198,51,100,1,16153>>8,16153&255};
    assert(!memcmp(sent,expected,sizeof expected));
}
// Main owns the copy while a background callback appends the next message.
static size_t binary_ws(uint8_t *wire,uint8_t kind,const uint8_t *payload,size_t n){
    size_t body=n+6,k=0;assert(body<65536);
    wire[k++]=0x82;wire[k++]=126;wire[k++]=(uint8_t)(body>>8);wire[k++]=(uint8_t)body;
    uint32_t app_length=(uint32_t)n+2;
    for(unsigned i=0;i<4;i++)wire[k++]=(uint8_t)(app_length>>(24-8*i));
    wire[k++]=1;wire[k++]=kind;memcpy(wire+k,payload,n);return k+n;
}
static void background_receive_ownership(void){
    uint8_t pixels[4210],wire[4300];char main_copy[4300],control[16];uint8_t kind;
    for(size_t i=0;i<sizeof pixels;i++)pixels[i]=(uint8_t)(i*37);
    size_t n=binary_ws(wire,18,pixels,sizeof pixels);
    feed(wire,7);assert(!pm_net_take(&kind,main_copy,sizeof main_copy));
    feed(wire+7,4093);assert(!pm_net_take(&kind,main_copy,sizeof main_copy));
    feed(wire+4100,n-4100);
    assert(pm_net_take(&kind,main_copy,sizeof main_copy));
    assert(kind==18 && pm_net_payload_length()==sizeof pixels && lwip_depth==0);
    assert(!memcmp(main_copy,pixels,sizeof pixels));
    // This emulates receive IRQ arrival while main holds a frame for SPI.
    const uint8_t reply[]={'{','}'};n=binary_ws(wire,8,reply,sizeof reply);feed(wire,n);
    assert(!memcmp(main_copy,pixels,sizeof pixels));
    assert(pm_net_take(&kind,control,sizeof control));
    assert(kind==8 && pm_net_payload_length()==2 && !strcmp(control,"{}"));
    assert(!pm_net_take(&kind,control,sizeof control) && lwip_depth==0);
    puts("background receive: SDK-lock protected calls, split >4KiB binary with NUL, immutable main/SPI copy PASS");
}
#if PM_TRACE_STATS
static void ready_again(void){
    authenticated();const uint8_t reply[]={5,0,0,1,0,0,0,0,0,0};feed(reply,sizeof reply);
    char response[256];int n=snprintf(response,sizeof response,"HTTP/1.1 101 Switching Protocols\r\nSec-WebSocket-Accept: %s\r\n\r\n",expected_accept);
    assert(n>0 && (size_t)n<sizeof response);feed((const uint8_t *)response,(size_t)n);assert(stage==PM_NET_READY);
    socket_instance.rcv_wnd=socket_instance.rcv_ann_wnd=23360;
}
static pm_rx_record rx_record(unsigned slot){pm_rx_record r;assert(pm_rx_snapshot_record(slot,&r));return r;}
static void receive_statistics(void){
    socket_instance.rcv_wnd=socket_instance.rcv_ann_wnd=23360;
    pm_rx_capture(clock_us);uint8_t payload[32]={0},wire[64],kind;char decoded[64];
    size_t n=binary_ws(wire,18,payload,sizeof payload);unsigned credit_before=credits_returned;
    pm_net_rx_frame_start(901,7);feed(wire,n);assert(pm_net_take(&kind,decoded,sizeof decoded));
    clock_us+=3000000;pm_net_poll();feed(wire,n);assert(pm_net_take(&kind,decoded,sizeof decoded));
    pm_rx_progress(64);pm_net_rx_frame_end(PM_RX_COMPLETE,0);pm_rx_capture(clock_us);
    pm_rx_record r=rx_record(0);assert(r.frame==901 && r.session==7 && r.callbacks==2);
    assert(r.gap_to-r.gap_from==3000000 && r.gap_anchor && !r.gap_tail && r.gap_rx1==0);
    assert(r.bytes==2*n && r.free_calls==2 && r.freed==2 && r.assembled==64);
    assert(r.early_bytes+r.consume_credit==credits_returned-credit_before && !r.after.rx && !r.after.credit);
    assert(r.before.alloc.flags==15 && r.before.alloc.bits==16);
    // A consumer stall with a near-full RX reserve and real early/deferred credit calls.
    pm_net_rx_frame_start(902,7);static uint8_t backlog[30000];feed(backlog,sizeof backlog);
    cyw43_arch_lwip_begin();consume(2000);cyw43_arch_lwip_end();
    clock_us+=50000;pool_stats.err++;seg_stats.err++;lwip_stats.mem.err++;lwip_stats.tcp.drop++;
    pm_net_poll();uint64_t first=clock_us;clock_us+=3000000;
    cyw43_arch_lwip_begin();assert(recv_cb(NULL,&socket_instance,NULL,ERR_OK)==ERR_OK);cyw43_arch_lwip_end();
    pm_rx_capture(clock_us);r=rx_record(1);assert(r.frame==902 && r.reason==PM_RX_PEER_CLOSED);
    assert(r.rx_peak==30000 && r.credit_peak==20592 && r.consume_credit==2000);
    assert(r.after.rx==28000 && r.after.credit==18592 && r.failure_mask==75 && r.first_failure==first);
    pm_net_close();ready_again();pm_rx_capture(clock_us);
    // The callback that overflows the bounded RX is counted and its pbuf is freed.
    pm_net_rx_frame_start(903,8);feed(backlog,sizeof backlog);feed(backlog,3000);assert(stage==PM_NET_FAILED);
    pm_rx_capture(clock_us);r=rx_record(1);assert(r.frame==903 && r.reason==PM_RX_OVERFLOW);
    assert(r.callbacks==2 && r.bytes==33000 && r.free_calls==2 && r.freed==2);
    pm_net_close();ready_again();pm_rx_capture(clock_us);
    // Allocation failures remain observable before any FrameStart is parsed.
    clock_us+=50000;pool_stats.err++;pm_net_poll();clock_us+=2000000;pm_net_rx_snapshot();pm_rx_capture(clock_us);
    r=rx_record(3);assert(r.frame==0 && r.reason==PM_RX_IDLE && (r.failure_mask&1) && r.end-r.start>=2050000);
    puts("production RX statistics: 3s arrival gap vs consumer backlog, exact credit/free accounting, pool/heap/TCP_SEG failures, overflow callback and pre-FrameStart idle PASS");
}
#endif
int main(void){
    authenticated();
    assert(!pm_net_proxy_target_connected());
    const uint8_t reply[]={5,0,0,1,0,0,0,0,0,0};
    unsigned before=sends;
    feed(reply,4);assert(stage==PM_NET_SOCKS && sends==before);
    feed(reply+4,6);assert(stage==PM_NET_WS && sent_length>20 && !memcmp(sent,"GET /device ",12));
    assert(pm_net_proxy_target_connected());
    char response[256];int n=snprintf(response,sizeof response,
        "HTTP/1.1 101 Switching Protocols\r\nSec-WebSocket-Accept: %s\r\n\r\n",expected_accept);
    assert(n>0 && (size_t)n<sizeof response);
    feed((const uint8_t *)response,11);assert(stage==PM_NET_WS);
    feed((const uint8_t *)response+11,(size_t)n-11);assert(stage==PM_NET_READY);
#if PM_TRACE_ENABLE
    for(unsigned i=0;i<100;i++)pm_trace_flush();
    trace_used=0;trace_text[0]=0;
    pm_trace_frame_context(255);output_result=-5;
    assert(pm_net_send(8,"{}") && stage==PM_NET_READY);output_result=0;
    pm_trace_frame_context(0);for(unsigned i=0;i<100;i++)pm_trace_flush();
#if PM_TRACE_VERBOSE
    assert(strstr(trace_text,"event=tcp_output frame=255 a=-5 b=60000"));
    assert(strstr(trace_text,"event=tcp_write frame=255 a=0"));
    puts("TCP output trace: actual error and duration retained; queue acceptance != wire arrival PASS");
#else
    assert(!strstr(trace_text,"event=tcp_output") && !strstr(trace_text,"event=tcp_write"));
    assert(pm_stats_read(false,PM_STAT_NET_SEND).calls>0);
    assert(pm_stats_read(false,PM_STAT_NET_SEND).max_us>=60000);
    puts("TCP quiet statistics: duration retained in RAM without per-message output PASS");
#endif
#endif
    background_receive_ownership();
    unsigned saved_room=send_room;send_room=0;
    assert(!pm_net_send_optional(19,"{}") && pm_net_state()==PM_NET_READY);
    send_room=saved_room;
    // A single WS body can exceed APP_CAP while individual legacy messages fit.
    uint8_t large[28000];char first[15001],second[11001],decoded[16000];uint8_t kind;
    memset(first,'a',15000);first[15000]=0;memset(second,'b',11000);second[11000]=0;
    size_t body=15006+11006,k=0;large[k++]=0x82;large[k++]=126;large[k++]=(uint8_t)(body>>8);large[k++]=(uint8_t)body;
    const char *parts[]={first,second};size_t sizes[]={15000,11000};
    for(unsigned part=0;part<2;part++){
        uint32_t n=(uint32_t)sizes[part]+2;
        for(unsigned b=0;b<4;b++)large[k++]=(uint8_t)(n>>(24-8*b));
        large[k++]=1;large[k++]=13;memcpy(large+k,parts[part],sizes[part]);k+=sizes[part];
    }
    feed(large,3000);assert(!pm_net_take(&kind,decoded,sizeof decoded));
    feed(large+3000,k-3000);
    assert(pm_net_take(&kind,decoded,sizeof decoded)&&kind==13&&!strcmp(decoded,first));
    assert(pm_net_take(&kind,decoded,sizeof decoded)&&kind==13&&!strcmp(decoded,second));
    assert(!pm_net_take(&kind,decoded,sizeof decoded)&&pm_net_state()==PM_NET_READY);
    puts("streaming WS decoder: concatenated >APP_CAP body and partial TCP preserve both legacy messages PASS");
    puts("production SOCKS5: split METHOD v5 / AUTH v1 / CONNECT v5 / WebSocket accepted");
#if PM_TRACE_STATS
    receive_statistics();
#endif
    write_result=-1;assert(!pm_net_send(5,"{}") && stage==PM_NET_FAILED);write_result=0;
    stage=PM_NET_READY;send_room=0;assert(!pm_net_send(5,"{}") && stage==PM_NET_FAILED);send_room=sizeof sent;
    puts("TCP send failure diagnostics: SDK error/full buffer preserve failure behavior");
    pm_net_close();assert(stage==PM_NET_DOWN && !rx_used && !app_used && !ws_continuation);
    begin();const uint8_t wrong_method[]={5,0};feed(wrong_method,2);assert(stage==PM_NET_FAILED);
    begin();const uint8_t method[]={5,2};feed(method,2);
    const uint8_t wrong_auth[]={1,1};feed(wrong_auth,2);assert(stage==PM_NET_FAILED);
    assert(!pm_net_proxy_target_connected() && pm_net_last_failure()==PM_NET_FAIL_PROTOCOL);
    authenticated();const uint8_t rejected[]={5,5,0,1};feed(rejected,4);assert(stage==PM_NET_FAILED);
    assert(!pm_net_proxy_target_connected() && pm_net_last_failure()==PM_NET_FAIL_TARGET);
    authenticated();const uint8_t malformed[]={5,0,1,1};feed(malformed,4);assert(stage==PM_NET_FAILED);
    assert(!pm_net_proxy_target_connected() && pm_net_last_failure()==PM_NET_FAIL_PROTOCOL);
    begin();error_cb(NULL,-14);pm_net_poll();
    assert(pm_net_last_failure()==PM_NET_FAIL_PROTOCOL);
    pm_net_begin(&test_profile);error_cb(NULL,-14);pm_net_poll();
    assert(pm_net_last_failure()==PM_NET_FAIL_TCP);
    puts("production SOCKS5: rejected method/auth/CONNECT and malformed reply fail; reconnect resets state");
    return 0;
}
