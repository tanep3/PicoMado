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
static struct tcp_pcb socket_instance;
static tcp_connected_fn connect_callback;
static tcp_recv_fn receive_callback;
static uint8_t sent[4096];
static size_t sent_length;
static unsigned sends;
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
void cyw43_arch_poll(void){}
uint32_t get_rand_32(void){return 0x12345678;}
int ipaddr_aton(const char *text,ip_addr_t *address){return inet_pton(AF_INET,text,&address->addr)==1;}
struct tcp_pcb *tcp_new_ip_type(int t){assert(t==IPADDR_TYPE_V4);return &socket_instance;}
void tcp_arg(struct tcp_pcb *p,void *arg){assert(p==&socket_instance && !arg);}
void tcp_recv(struct tcp_pcb *p,tcp_recv_fn cb){assert(p==&socket_instance);receive_callback=cb;}
void tcp_err(struct tcp_pcb *p,tcp_err_fn cb){assert(p==&socket_instance);(void)cb;}
err_t tcp_connect(struct tcp_pcb *p,const ip_addr_t *a,uint16_t port,tcp_connected_fn cb){
    assert(p==&socket_instance && a && port==1080);connect_callback=cb;return ERR_OK;
}
unsigned tcp_sndbuf(struct tcp_pcb *p){assert(p==&socket_instance);return send_room;}
err_t tcp_write(struct tcp_pcb *p,const void *bytes,size_t n,int flags){
    assert(p==&socket_instance && n<=sizeof sent && flags==TCP_WRITE_FLAG_COPY);
    if(write_result)return write_result;
    memcpy(sent,bytes,n);sent_length=n;sends++;return ERR_OK;
}
err_t tcp_output(struct tcp_pcb *p){assert(p==&socket_instance);
#if PM_TRACE_ENABLE
    clock_us+=60000;
#endif
    return output_result;
}
err_t tcp_close(struct tcp_pcb *p){assert(p==&socket_instance);return ERR_OK;}
void tcp_abort(struct tcp_pcb *p){assert(p==&socket_instance);}
void tcp_recved(struct tcp_pcb *p,u16_t n){assert(p==&socket_instance);(void)n;}
unsigned pbuf_copy_partial(const struct pbuf *p,void *out,unsigned n,unsigned offset){
    assert(offset+n<=p->tot_len);memcpy(out,p->payload+offset,n);return n;
}
void pbuf_free(struct pbuf *p){assert(p);}
static void feed(const uint8_t *data,size_t n){
    struct pbuf buffer={(unsigned)n,data};assert(receive_callback);
    assert(receive_callback(NULL,&socket_instance,&buffer,ERR_OK)==ERR_OK);pm_net_poll();
}
static void begin(void){
    pm_net_begin(&test_profile);assert(stage==PM_NET_TCP);
    assert(connect_callback(NULL,&socket_instance,ERR_OK)==ERR_OK);
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
int main(void){
    authenticated();
    const uint8_t reply[]={5,0,0,1,0,0,0,0,0,0};
    unsigned before=sends;
    feed(reply,4);assert(stage==PM_NET_SOCKS && sends==before);
    feed(reply+4,6);assert(stage==PM_NET_WS && sent_length>20 && !memcmp(sent,"GET /device ",12));
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
    assert(strstr(trace_text,"event=tcp_output frame=255 a=-5 b=60000"));
    assert(strstr(trace_text,"event=tcp_write frame=255 a=0"));
    puts("TCP output trace: actual error and duration retained; queue acceptance != wire arrival PASS");
#endif
    puts("production SOCKS5: split METHOD v5 / AUTH v1 / CONNECT v5 / WebSocket accepted");
    write_result=-1;assert(!pm_net_send(5,"{}") && stage==PM_NET_FAILED);write_result=0;
    stage=PM_NET_READY;send_room=0;assert(!pm_net_send(5,"{}") && stage==PM_NET_FAILED);send_room=sizeof sent;
    puts("TCP send failure diagnostics: SDK error/full buffer preserve failure behavior");
    pm_net_close();assert(stage==PM_NET_DOWN && !rx_used && !app_used && !ws_continuation);
    begin();const uint8_t wrong_method[]={5,0};feed(wrong_method,2);assert(stage==PM_NET_FAILED);
    begin();const uint8_t method[]={5,2};feed(method,2);
    const uint8_t wrong_auth[]={1,1};feed(wrong_auth,2);assert(stage==PM_NET_FAILED);
    authenticated();const uint8_t rejected[]={5,5,0,1};feed(rejected,4);assert(stage==PM_NET_FAILED);
    authenticated();const uint8_t malformed[]={5,0,1,1};feed(malformed,4);assert(stage==PM_NET_FAILED);
    puts("production SOCKS5: rejected method/auth/CONNECT and malformed reply fail; reconnect resets state");
    return 0;
}
