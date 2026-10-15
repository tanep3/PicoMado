#include "trace.h"
#include "net.h"
#include "ws_accept.h"
#include "rx_flow.h"
#include "socks5_wire.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "pico/stdlib.h"
#include "pico/rand.h"
#include "pico/cyw43_arch.h"
#include "lwip/ip_addr.h"
#include "lwip/tcp.h"

// One server image fragment is <17 KiB after JSON encoding. The TCP window
// must hold a whole fragment because receive credit is returned after parsing.
#define RX_CAP 32768u
// A 4096-byte JSON integer array needs at most four characters per byte.
// Reserve room for its envelope and WebSocket header before granting TCP credit.
_Static_assert(TCP_WND > 4096u * 4u + 512u, "TCP window must fit one image fragment");
_Static_assert(RX_CAP > TCP_WND, "receive buffer must exceed TCP window");
static struct tcp_pcb *pcb;
static const pm_profile *profile;
static pm_net_stage stage;
static enum { SOCKS_METHOD,SOCKS_AUTH,SOCKS_CONNECT } socks_step;
static uint8_t rx[RX_CAP];
static pm_rx_flow rx_flow;
#define rx_used rx_flow.used
// A legacy numeric-array chunk is <18KiB. Stream a WS body through this
// bounded application workspace; never require a whole concatenated WS body.
#define APP_CAP 18006u
_Static_assert(APP_CAP>4096u*4u+1024u,"legacy chunk workspace");
static uint8_t app_rx[APP_CAP];
static size_t ws_remaining;
static size_t app_used,last_payload_length;
static bool ws_continuation;
static bool socket_dead;
static char expected_accept[29];
// The RP2040 main stack is 2 KiB. Keep large protocol workspaces in BSS.
static char ws_header[2049];
static uint8_t tx_packet[1040];
static char upgrade_request[320];
static uint8_t socks_auth_request[513];
static ip_addr_t server_addr, proxy_addr;

static void consume(size_t n) {
    size_t credit=0;
    if (!pm_rx_flow_consume(&rx_flow,n,&credit)) { stage=PM_NET_FAILED; return; }
    memmove(rx,rx+n,rx_used);
    if(pcb && credit)tcp_recved(pcb,(u16_t)credit);
}
static bool send_raw(const void *p, size_t n) {
    if(!pcb){pm_logf("%s\n","net send_failed=no_socket");stage=PM_NET_FAILED;return false;}
    if(n>tcp_sndbuf(pcb)){
        pm_logf("%s\n","net send_failed=send_buffer_full");stage=PM_NET_FAILED;return false;
    }
    uint64_t write_started=pm_trace_clock();
    err_t result=tcp_write(pcb,p,n,TCP_WRITE_FLAG_COPY);
    pm_trace_event(PM_TRACE_TCP_WRITE,pm_trace_current_frame(),result,
        (uint32_t)(pm_trace_clock()-write_started));
    if(result!=ERR_OK){
        pm_logf("net tcp_write_failed sdk_result=%d\n",(int)result);
        stage=PM_NET_FAILED; return false;
    }
    uint64_t output_started=pm_trace_clock();
    err_t output_result=tcp_output(pcb);
    pm_trace_event(PM_TRACE_TCP_OUTPUT,pm_trace_current_frame(),output_result,
        (uint32_t)(pm_trace_clock()-output_started));
    // Preserve existing queueing semantics; output result is diagnostic, not arrival ACK.
    return true;
}
static void error_cb(void *arg, err_t e) {
    (void)arg;pm_logf("net tcp_error sdk_result=%d\n",(int)e);pcb=NULL;socket_dead=true;
}
static err_t recv_cb(void *arg, struct tcp_pcb *p, struct pbuf *buf, err_t e) {
    (void)arg;(void)p;
    if (!buf) { pm_logf("%s\n","net tcp_peer_closed");socket_dead=true;return ERR_OK; }
    if (e!=ERR_OK || buf->tot_len>RX_CAP-rx_used) {
        pm_logf("net receive_failed sdk_result=%d used=%u incoming=%u\n",(int)e,(unsigned)rx_used,(unsigned)buf->tot_len);
        pbuf_free(buf); socket_dead=true; return ERR_OK;
    }
    size_t n=buf->tot_len, credit=0;
    pbuf_copy_partial(buf,rx+rx_used,n,0);
    pbuf_free(buf);
    if(!pm_rx_flow_append(&rx_flow,n,RX_CAP,TCP_WND,&credit)) {
        socket_dead=true; return ERR_OK;
    }
    // The copied bytes are safe in RX_CAP. Hold only the final window-sized
    // reserve, so receiving a partial frame never depends on parsing it first.
    if(credit)tcp_recved(p,(u16_t)credit);
    return ERR_OK;
}
static bool upgrade(void) {
    char key[25];
    static const char b64[]="ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    uint8_t seed[16];
    for(unsigned i=0;i<16;i+=4) {
        uint32_t r=get_rand_32();
        for(unsigned j=0;j<4;j++)seed[i+j]=(uint8_t)(r>>(24-8*j));
    }
    for(unsigned i=0,j=0;i<16;i+=3) {
        uint32_t x=(uint32_t)seed[i]<<16;
        if(i+1<16)x|=(uint32_t)seed[i+1]<<8;
        if(i+2<16)x|=seed[i+2];
        key[j++]=b64[(x>>18)&63]; key[j++]=b64[(x>>12)&63];
        key[j++]=i+1<16?b64[(x>>6)&63]:'=';
        key[j++]=i+2<16?b64[x&63]:'=';
    }
    key[24]=0;
    if(!pm_ws_accept(key,expected_accept))return false;
    int n=snprintf(upgrade_request,sizeof upgrade_request,
        "GET /device HTTP/1.1\r\nHost: %s:16153\r\nUpgrade: websocket\r\n"
        "Connection: Upgrade\r\nSec-WebSocket-Key: %s\r\n"
        "Sec-WebSocket-Version: 13\r\n\r\n",profile->server_ip,key);
    if(n<=0 || (size_t)n>=sizeof upgrade_request) return false;
    stage=PM_NET_WS; return send_raw(upgrade_request,(size_t)n);
}
static err_t connected_cb(void *arg, struct tcp_pcb *p, err_t e) {
    (void)arg;(void)p;
    if(e!=ERR_OK){pm_logf("net tcp_connect_failed sdk_result=%d\n",(int)e);socket_dead=true;return e;}
    if(!profile->socks) { if(!upgrade())socket_dead=true; return ERR_OK; }
    const uint8_t greeting[]={5,1,2};
    socks_step=SOCKS_METHOD;
    stage=PM_NET_SOCKS;
    if(!send_raw(greeting,sizeof greeting))socket_dead=true;
    return ERR_OK;
}
bool pm_net_init(void) {
    return cyw43_arch_init()==0;
}
static void close_unlocked(void) {
    if(pcb) {
        struct tcp_pcb *old=pcb; pcb=NULL;
        tcp_arg(old,NULL); tcp_recv(old,NULL); tcp_err(old,NULL);
        if(tcp_close(old)!=ERR_OK)tcp_abort(old);
    }
    stage=PM_NET_DOWN;socks_step=SOCKS_METHOD;
    pm_rx_flow_reset(&rx_flow); app_used=0; last_payload_length=0; ws_continuation=false; ws_remaining=0; socket_dead=false;
}
static void begin_unlocked(const pm_profile *p) {
    close_unlocked(); profile=p;
    if(!p || !p->server_ip || !ipaddr_aton(p->server_ip,&server_addr)) { stage=PM_NET_FAILED;return; }
    if(p->socks && (!p->proxy_ip || !ipaddr_aton(p->proxy_ip,&proxy_addr) ||
                    !p->proxy_user || !p->proxy_password ||
                    !p->proxy_user[0] || !p->proxy_password[0] ||
                    strlen(p->proxy_user)>255 || strlen(p->proxy_password)>255)) {
        stage=PM_NET_FAILED;return;
    }
    pcb=tcp_new_ip_type(IPADDR_TYPE_V4);
    if(!pcb){stage=PM_NET_FAILED;return;}
    pcb->so_options|=SOF_KEEPALIVE;
    pcb->keep_idle=30000;pcb->keep_intvl=5000;pcb->keep_cnt=3;
    tcp_arg(pcb,NULL);tcp_recv(pcb,recv_cb);tcp_err(pcb,error_cb);
    stage=PM_NET_TCP;
    if(tcp_connect(pcb,p->socks?&proxy_addr:&server_addr,
        p->socks?p->proxy_port:16153,connected_cb)!=ERR_OK)stage=PM_NET_FAILED;
}

static void process_socks(void) {
    if(socks_step==SOCKS_METHOD){
        int result=socks5_method_result(rx,rx_used);
        if(result==SOCKS5_INCOMPLETE)return;
        pm_logf("net socks_method result=%d\n",result);
        if(result!=SOCKS5_OK){stage=PM_NET_FAILED;return;}
        consume(2);
        size_t n=0;
        if(!socks5_make_auth(profile->proxy_user,profile->proxy_password,
            socks_auth_request,sizeof socks_auth_request,&n)){stage=PM_NET_FAILED;return;}
        socks_step=SOCKS_AUTH;
        bool sent=send_raw(socks_auth_request,n);
        memset(socks_auth_request,0,n); // tcp_write used COPY; clear credential copy.
        if(!sent)stage=PM_NET_FAILED;
        return;
    }
    if(socks_step==SOCKS_AUTH){
        int result=socks5_auth_result(rx,rx_used);
        if(result==SOCKS5_INCOMPLETE)return;
        pm_logf("net socks_auth result=%d\n",result);
        if(result!=SOCKS5_OK){stage=PM_NET_FAILED;return;}
        consume(2);
        uint8_t req[10];
        const ip4_addr_t *v=ip_2_ip4(&server_addr);
        socks5_make_connect_ipv4((const uint8_t *)&v->addr,16153,req);
        socks_step=SOCKS_CONNECT;
        if(!send_raw(req,sizeof req))stage=PM_NET_FAILED;
        return;
    }
    size_t n=0;
    int result=socks5_connect_result(rx,rx_used,&n);
    if(result==SOCKS5_INCOMPLETE)return;
    pm_logf("net socks_connect result=%d\n",result);
    if(result!=SOCKS5_OK){stage=PM_NET_FAILED;return;}
    consume(n);if(!upgrade())stage=PM_NET_FAILED;
}
static void poll_unlocked(void) {
    uint64_t started=pm_trace_clock();
    pm_trace_slow(PM_TRACE_SLOW_POLL,started,2);
    if(socket_dead){close_unlocked();stage=PM_NET_FAILED;return;}
    if(stage==PM_NET_SOCKS && rx_used)process_socks();
    if(stage==PM_NET_WS) {
        static const uint8_t end[]={"\r\n\r\n"};
        for(size_t i=0;i+4<=rx_used;i++) if(!memcmp(rx+i,end,4)) {
            if(rx_used<12 || memcmp(rx,"HTTP/1.1 101",12)) {stage=PM_NET_FAILED;return;}
            // Limit the header scan to the terminated HTTP header, not binary payload.
            if(i+4>=sizeof ws_header){stage=PM_NET_FAILED;return;}
            memcpy(ws_header,rx,i+4);ws_header[i+4]=0;
            bool accepted=false;
            const char *line=ws_header;
            while((line=strstr(line,"\r\n"))!=NULL){
                line+=2;
                static const char name[]="Sec-WebSocket-Accept:";
                size_t j=0;
                for(;j<sizeof name-1 && line[j];j++){
                    char c=line[j],want=name[j];
                    if(c>='A'&&c<='Z')c+=32;
                    if(want>='A'&&want<='Z')want+=32;
                    if(c!=want)break;
                }
                if(j==sizeof name-1){
                    const char *v=line+j;
                    while(*v==' '||*v=='\t')v++;
                    const char *eol=strstr(v,"\r\n");
                    accepted=eol && eol-v==28 && !strncmp(v,expected_accept,28);
                    break;
                }
            }
            if(!accepted){stage=PM_NET_FAILED;return;}
            consume(i+4);stage=PM_NET_READY;return;
        }
        if(rx_used>2048)stage=PM_NET_FAILED;
    }
}
static pm_net_stage state_unlocked(void){return stage;}

static bool send_unlocked(uint8_t kind, const char *json) {
    if(stage!=PM_NET_READY || !pcb)return false;
    size_t n=strlen(json);if(n>1024)return false;
    uint8_t *packet=tx_packet;size_t k=0;
    packet[k++]=0x82;
    size_t len=n+6;
    if(len<126)packet[k++]=0x80|(uint8_t)len;
    else {packet[k++]=0xfe;packet[k++]=(uint8_t)(len>>8);packet[k++]=(uint8_t)len;}
    uint32_t random=get_rand_32();
    uint8_t mask[4]={random>>24,random>>16,random>>8,random};
    memcpy(packet+k,mask,4);k+=4;
    uint8_t header[6]={(uint8_t)((n+2)>>24),(uint8_t)((n+2)>>16),
        (uint8_t)((n+2)>>8),(uint8_t)(n+2),1,kind};
    for(size_t i=0;i<6;i++)packet[k++]=header[i]^mask[i&3];
    for(size_t i=0;i<n;i++)packet[k++]=(uint8_t)json[i]^mask[(i+6)&3];
    return send_raw(packet,k);
}

size_t pm_net_payload_length(void){return last_payload_length;}
static bool take_unlocked(uint8_t *kind, char *json, size_t cap) {
again:
    if(stage!=PM_NET_READY)return false;
    if(app_used>=4) {
        uint32_t n=(uint32_t)app_rx[0]<<24|(uint32_t)app_rx[1]<<16|
            (uint32_t)app_rx[2]<<8|app_rx[3];
        if(n<2 || n>sizeof app_rx-4 || (n>4096 && app_used>=6 && app_rx[5]!=13 && app_rx[5]!=18)) {
            stage=PM_NET_FAILED;return false;
        }
        if(app_used>=n+4) {
            if(app_rx[4]!=1 || n-2>=cap){stage=PM_NET_FAILED;return false;}
            if(app_rx[5]<1 || app_rx[5]>18){stage=PM_NET_FAILED;return false;}
            last_payload_length=n-2;*kind=app_rx[5];memcpy(json,app_rx+6,n-2);json[n-2]=0;
            app_used-=n+4;memmove(app_rx,app_rx+n+4,app_used);return true;
        }
    }
    if(ws_remaining){
        size_t n=rx_used;if(n>ws_remaining)n=ws_remaining;
        if(n>sizeof app_rx-app_used)n=sizeof app_rx-app_used;
        if(!n)return false;
        memcpy(app_rx+app_used,rx,n);app_used+=n;ws_remaining-=n;consume(n);
        goto again;
    }
    if(rx_used<2)return false;
    unsigned opcode=rx[0]&15;
    bool fin=(rx[0]&0x80)!=0;
    if(rx[1]&0x80) {stage=PM_NET_FAILED;return false;}
    uint64_t length=rx[1]&127;size_t h=2;
    if(length==126){if(rx_used<4)return false;length=((unsigned)rx[2]<<8)|rx[3];h=4;}
    else if(length==127){
        if(rx_used<10)return false;
        length=0;for(unsigned i=2;i<10;i++)length=length<<8|rx[i];h=10;
    }
    if(length>RX_CAP-10 || length+ h >RX_CAP) {stage=PM_NET_FAILED;return false;}
    if((opcode==8 || opcode==9) && rx_used<h+length)return false;
    if(opcode==8){stage=PM_NET_FAILED;return false;}
    if(opcode==9){
        // Server ping is not an application message. Reply with a masked pong.
        if(length>125){stage=PM_NET_FAILED;return false;}
        uint8_t pong[131];size_t p=0;pong[p++]=0x8a;pong[p++]=0x80|(uint8_t)length;
        uint32_t random=get_rand_32();
        uint8_t mask[4]={random>>24,random>>16,random>>8,random};
        memcpy(pong+p,mask,4);p+=4;
        for(size_t i=0;i<length;i++)pong[p++]=rx[h+i]^mask[i&3];
        send_raw(pong,p);consume(h+(size_t)length);return false;
    }
    if((opcode!=2 && opcode!=0) || (opcode==0 && !ws_continuation) ||
       (opcode==2 && ws_continuation)) {
        stage=PM_NET_FAILED;return false;
    }
    ws_continuation=!fin;
    consume(h);ws_remaining=(size_t)length;
    goto again;
}

// Background IRQ owns RX writes; main owns app/pixel decoding. Copy/consume
// transport buffers under the SDK lock, then release it before rendering.
void pm_net_close(void){cyw43_arch_lwip_begin();close_unlocked();cyw43_arch_lwip_end();}
void pm_net_begin(const pm_profile *p){cyw43_arch_lwip_begin();begin_unlocked(p);cyw43_arch_lwip_end();}
void pm_net_poll(void){cyw43_arch_poll();cyw43_arch_lwip_begin();poll_unlocked();cyw43_arch_lwip_end();}
pm_net_stage pm_net_state(void){cyw43_arch_lwip_begin();pm_net_stage result=state_unlocked();cyw43_arch_lwip_end();return result;}
bool pm_net_send(uint8_t kind,const char *json){cyw43_arch_lwip_begin();bool ok=send_unlocked(kind,json);cyw43_arch_lwip_end();return ok;}
bool pm_net_take(uint8_t *kind,char *json,size_t cap){cyw43_arch_lwip_begin();bool ok=take_unlocked(kind,json,cap);cyw43_arch_lwip_end();return ok;}

// Optional video telemetry must not turn send-buffer pressure into a retry.
bool pm_net_send_optional(uint8_t kind,const char *json){
    cyw43_arch_lwip_begin();
    bool room=stage==PM_NET_READY && pcb && strlen(json)+14<=tcp_sndbuf(pcb);
    bool ok=room && send_unlocked(kind,json);cyw43_arch_lwip_end();return ok;
}
