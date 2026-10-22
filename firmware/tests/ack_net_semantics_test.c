// Production connection/handshake/control paths with deterministic sockets.
// No device, network, credential, or operating-system settings are accessed.
#include <arpa/inet.h>
#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "core.h"
#ifndef PM_NET_SOURCE
#define PM_NET_SOURCE "../src/net.c"
#endif
#ifndef PM_EXPECT_LCD_NODELAY
#define PM_EXPECT_LCD_NODELAY 1
#endif
#ifndef PM_EXPECT_EINK_NODELAY
#define PM_EXPECT_EINK_NODELAY 1
#endif
#include PM_NET_SOURCE

cyw43_t cyw43_state;
static struct tcp_pcb socket_instance;
static tcp_connected_fn on_connect;
static tcp_recv_fn on_receive;
static unsigned lock_depth,connects,creates,outputs,closes,writes;
static err_t injected_write,injected_output;
static unsigned flags_at_connect;
static bool fail_alloc,fail_connect;
static const pm_profile *connecting_profile;
static uint8_t sent[2048];
static size_t sent_size;
static const pm_profile home={"test","test","192.0.2.1",false,NULL,NULL,NULL,0};
static const pm_profile away={"test","test","192.0.2.1",true,"198.51.100.2","test","test",1080};
static unsigned expected_flags(void){
    return ((PM_DISPLAY_TYPE==PM_DISPLAY_LCD && PM_EXPECT_LCD_NODELAY) ||
            (PM_DISPLAY_TYPE==PM_DISPLAY_EINK && PM_EXPECT_EINK_NODELAY)) ? TF_NODELAY : 0;
}
uint64_t time_us_64(void){return 1000000;}
uint32_t get_rand_32(void){return 0x12345678;}
int cyw43_arch_init(void){return 0;}
void cyw43_arch_poll(void){}
void cyw43_arch_lwip_begin(void){assert(!lock_depth);lock_depth=1;}
void cyw43_arch_lwip_end(void){assert(lock_depth==1);lock_depth=0;}
int ipaddr_aton(const char *text,ip_addr_t *address){return inet_pton(AF_INET,text,&address->addr)==1;}
struct tcp_pcb *tcp_new_ip_type(int type){
    assert(lock_depth==1 && type==IPADDR_TYPE_V4);creates++;
    if(fail_alloc)return NULL;
    // Real tcp_alloc zeroes each new PCB. Reuse this address to catch reliance
    // on a previous connection's flag rather than configuring the new PCB.
    memset(&socket_instance,0,sizeof socket_instance);return &socket_instance;
}
void tcp_arg(struct tcp_pcb *p,void *arg){assert(lock_depth==1 && p==&socket_instance && !arg);}
void tcp_recv(struct tcp_pcb *p,tcp_recv_fn cb){assert(lock_depth==1 && p==&socket_instance);on_receive=cb;}
void tcp_err(struct tcp_pcb *p,tcp_err_fn cb){assert(lock_depth==1 && p==&socket_instance);(void)cb;}
err_t tcp_connect(struct tcp_pcb *p,const ip_addr_t *address,uint16_t port,tcp_connected_fn cb){
    assert(lock_depth==1 && p==&socket_instance && address && cb);
    ip_addr_t expected;assert(ipaddr_aton(connecting_profile->socks?connecting_profile->proxy_ip:connecting_profile->server_ip,&expected));
    assert(address->addr==expected.addr && port==(connecting_profile->socks?1080:16153));
    flags_at_connect=p->flags;assert((flags_at_connect&TF_NODELAY)==expected_flags());
    assert(p->so_options&SOF_KEEPALIVE);assert(p->keep_idle==30000 && p->keep_intvl==5000 && p->keep_cnt==3);
    connects++;on_connect=cb;return fail_connect?-5:ERR_OK;
}
unsigned tcp_sndbuf(struct tcp_pcb *p){assert(lock_depth==1 && p==&socket_instance);return 65535;}
err_t tcp_write(struct tcp_pcb *p,const void *bytes,size_t length,int flags){
    assert(lock_depth==1 && p==&socket_instance && flags==TCP_WRITE_FLAG_COPY && length<=sizeof sent);
    assert((p->flags&TF_NODELAY)==expected_flags());if(injected_write)return injected_write;writes++;p->snd_lbb+=(uint32_t)length;memcpy(sent,bytes,length);sent_size=length;return ERR_OK;
}
err_t tcp_output(struct tcp_pcb *p){assert(lock_depth==1 && p==&socket_instance);outputs++;return injected_output;}
err_t tcp_close(struct tcp_pcb *p){assert(lock_depth==1 && p==&socket_instance);closes++;return ERR_OK;}
void tcp_abort(struct tcp_pcb *p){assert(lock_depth==1 && p==&socket_instance);}
void tcp_recved(struct tcp_pcb *p,u16_t size){assert(lock_depth==1 && p==&socket_instance);(void)size;}
unsigned pbuf_copy_partial(const struct pbuf *p,void *out,unsigned size,unsigned offset){
    assert(offset+size<=p->tot_len);memcpy(out,p->payload+offset,size);return size;
}
unsigned char pbuf_free(struct pbuf *p){assert(p);return 1;}
static void feed(const void *bytes,unsigned size){
    struct pbuf p={size,bytes};assert(on_receive);cyw43_arch_lwip_begin();
    assert(on_receive(NULL,&socket_instance,&p,ERR_OK)==ERR_OK);cyw43_arch_lwip_end();pm_net_poll();
}
static void ready(const pm_profile *profile){
    connecting_profile=profile;pm_net_begin(profile);assert(stage==PM_NET_TCP);
    cyw43_arch_lwip_begin();assert(on_connect(NULL,&socket_instance,ERR_OK)==ERR_OK);cyw43_arch_lwip_end();
    if(profile->socks){
        const uint8_t greeting[]={5,1,2};assert(sent_size==3 && !memcmp(sent,greeting,3));
        const uint8_t method[]={5,2},auth[]={1,0},reply[]={5,0,0,1,0,0,0,0,0,0};
        feed(method,sizeof method);assert(sent_size==11 && sent[0]==1);
        feed(auth,sizeof auth);assert(sent_size==10 && sent[0]==5 && sent[1]==1);
        feed(reply,sizeof reply);assert(pm_net_proxy_target_connected());
    }
    assert(stage==PM_NET_WS && sent_size>20 && !memcmp(sent,"GET /device ",12));
    char reply[256];int size=snprintf(reply,sizeof reply,"HTTP/1.1 101 Switching Protocols\r\nSec-WebSocket-Accept: %s\r\n\r\n",expected_accept);
    assert(size>0 && size<(int)sizeof reply);feed(reply,(unsigned)size);assert(stage==PM_NET_READY);
    // All application controls retain framing, kind and contents.
    const uint8_t kinds[]={6,8,9,19};
    for(unsigned i=0;i<sizeof kinds;i++){
        unsigned before=outputs;assert(pm_net_send(kinds[i],"{}"));assert(outputs==before+1);
        assert(sent_size==14 && sent[0]==0x82 && sent[1]==0x88);
        uint8_t decoded[8];for(unsigned j=0;j<8;j++)decoded[j]=sent[6+j]^sent[2+j%4];
        const uint8_t expected[]={0,0,0,4,1,kinds[i],'{','}'};assert(!memcmp(decoded,expected,8));
    }
    // WebSocket ping/pong still uses its existing masked control-frame path.
    const uint8_t ping[]={0x89,2,'O','K'};feed(ping,sizeof ping);
    uint8_t kind;char payload[16];assert(!pm_net_take(&kind,payload,sizeof payload));
    assert(sent_size==8 && sent[0]==0x8a && sent[1]==0x82);
    assert((sent[6]^sent[2])=='O' && (sent[7]^sent[3])=='K');
    pm_net_close();assert(stage==PM_NET_DOWN && !lock_depth);
}

static uint8_t wire_kind(void){size_t base=(sent[1]&127)==126?8:6;size_t mask=base-4;return sent[base+5]^sent[mask+1];}
int main(void){
 ready(&home);ready(&away);
 // Reuse the established fixture PCB, not an actual socket or device.
 pcb=&socket_instance;stage=PM_NET_READY;ws_closed=socket_dead=false;
 #if PM_ACK_PATH_PROBE
 pm_ack_probe_init_boot(1);pm_ack_probe_command("AP2 ARM 1 1");pm_ack_probe_button(1,1,1,1,false);pm_ack_probe_context(1,77);
#endif
 unsigned count=writes,before=outputs;
 injected_output=-12;
 assert(pm_net_send(9,"{}"));assert(writes==count+1&&outputs==before+1&&wire_kind()==9);
 assert(stage==PM_NET_READY); // tcp_write accepted a COPY despite failed output.
 injected_output=ERR_OK;
 assert(pm_net_send(15,"{}"));assert(writes==count+2&&wire_kind()==15);
 // No application replay of kind9 was issued; ordering remains lwIP's job.
 injected_write=ERR_MEM;before=outputs;
 assert(!pm_net_send(9,"{}"));assert(stage==PM_NET_FAILED&&outputs==before&&writes==count+2);
 assert(!pm_net_send(15,"{}"));assert(writes==count+2);
#if PM_ACK_PATH_PROBE
 bool output_error=false,write_error=false,position=false;pm_ack_probe_record record;
 for(unsigned i=0;pm_ack_probe_read(i,&record);i++){
  if(record.event==PM_AP_TX_POSITION){position=true;assert((uint32_t)(record.b-record.a)>0);}
  if(record.event==PM_AP_OUTPUT_END&&(int32_t)record.a==-12)output_error=true;
  if(record.event==PM_AP_WRITE_END&&(int32_t)record.a==ERR_MEM)write_error=true;
 }
 assert(output_error&&write_error&&position);
#endif
 fail_alloc=true;pm_net_begin(&home);assert(stage==PM_NET_FAILED);fail_alloc=false;
 puts("PASS production net semantics: output error preserves enqueue success; no app replay; write error fails transport and rejects following control");
 return 0;
}
