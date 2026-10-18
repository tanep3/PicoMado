// Production main helpers with the same physical/network mocks as the timing suite.
#define PM_APP_TEST_ENTRY unused_base_timing_test
#include "app_timing_test.c"
static pm_endpoint_network observed={true,0x0a010203,0x0a010201,0xffffff00};
static bool udp_open,udp_packet,proxy_connected;
static unsigned udp_opens,udp_sends,nonce_counter;
static pm_eex_message udp_incoming;
static pm_endpoint_network endpoint_network(void *p){(void)p;return observed;}
static bool endpoint_open(void *p,uint32_t local){(void)p;assert(!udp_open&&local==observed.ipv4);udp_open=true;udp_opens++;return true;}
static void endpoint_close(void *p){(void)p;assert(udp_open);udp_open=false;}
static size_t endpoint_receive(void *p,uint8_t *wire,size_t cap,pm_eex_peer *from){
    (void)p;if(!udp_packet)return 0;udp_packet=false;*from=(pm_eex_peer){0x0a010201,49000};
    return pm_eex_encode(&udp_incoming,(char *)wire,cap);
}
static bool endpoint_send(void *p,const uint8_t *wire,size_t n,pm_eex_peer to){
    (void)p;(void)to;pm_eex_message m;assert(pm_eex_parse(wire,n,&m));udp_sends++;return true;
}
static uint32_t endpoint_random(void *p){(void)p;return ++nonce_counter;}
pm_endpoint_io pm_endpoint_sdk_io(void){
    return (pm_endpoint_io){NULL,endpoint_network,endpoint_open,endpoint_close,endpoint_receive,endpoint_send,endpoint_random};
}
bool pm_net_proxy_target_connected(void){return proxy_connected;}
pm_net_failure pm_net_last_failure(void){return PM_NET_FAIL_TCP;}
static void app_offer(void){
    udp_incoming=endpoint_client.exchange.offer;udp_incoming.type=PM_EEX_O;
    memset(udp_incoming.offer,'b',32);udp_incoming.offer[32]=0;
    memset(udp_incoming.generation,'c',32);udp_incoming.generation[32]=0;
    udp_incoming.source_host=true;udp_incoming.port=48115;udp_incoming.ttl=30;udp_packet=true;
}
int main(void){
    defaults();memset(test_flash_sector,0xff,sizeof test_flash_sector);
    sta_on=wifi_enabled=true;active_profile=0;
    pm_endpoint_client_init(&endpoint_client,pm_endpoint_sdk_io());endpoint_initialized=true;
    start_wifi();wifi_link=ip_link=CYW43_LINK_UP;check_wifi_pm();start_server();
    assert(state==SERVER&&udp_opens==0&&server_begins==1&&joins==1&&pm_calls==1&&pm_reads==1);
    // Selecting AWAY starts a new Wi-Fi generation; app can be opened after the Pico.
    active_profile=1;start_wifi();wifi_link=ip_link=CYW43_LINK_UP;check_wifi_pm();start_server();
    assert(state==ENDPOINT&&server_begins==1&&udp_open&&joins==2&&pm_calls==2&&pm_reads==2);
    for(unsigned i=0;i<30;i++){clock_ms+=1000;poll_endpoint();}
    assert(state==ENDPOINT&&server_begins==1&&udp_sends>2);
    app_offer();poll_endpoint();assert(state==ENDPOINT);
    udp_incoming=endpoint_client.exchange.offer;udp_incoming.type=PM_EEX_K;udp_incoming.ack='A';udp_incoming.attempt=0;udp_packet=true;
    clock_ms++;poll_endpoint();
    assert(state==SERVER&&server_begins==2&&!strcmp(endpoint_profile.proxy_ip,"10.1.2.1")&&endpoint_profile.proxy_port==48115);
    assert(!endpoint_client.cache.valid);
    network_stage=PM_NET_WS;proxy_connected=true;endpoint_proxy_progress();proxy_connected=false;
    assert(endpoint_client.cache.valid&&endpoint_client.exchange.stage==PM_EEX_TARGET_CONNECTED);
    clock_ms+=1600;poll_endpoint();assert(!udp_open);
    unsigned opens=udp_opens;retry();clock_ms+=60000;start_server();
    assert(state==SERVER&&server_begins==3&&udp_opens==opens&&joins==2&&pm_calls==2);
    // DHCP change invalidates candidate and returns to existing healthy-WiFi retry.
    observed.gateway=0x0a010209;clock_ms+=300;poll_endpoint();
    assert(state==RETRY&&!endpoint_client.cache.valid&&joins==2);
    start_server();assert(state==ENDPOINT&&udp_open&&udp_opens==opens+1);
    app_offer();poll_endpoint();clock_ms+=1500;poll_endpoint();assert(state==SERVER);
    network_stage=PM_NET_FAILED;endpoint_proxy_progress();
    assert(endpoint_client.exchange.status==PM_EEX_FAIL&&endpoint_client.exchange.stage==PM_EEX_TCP_FAILED);
    retry();clock_ms+=1600;poll_endpoint();start_server();
    assert(state==ENDPOINT&&!endpoint_client.cache.valid&&joins==2);
    // Menu/standby disables discovery and preserves the pre-existing HOME path.
    menu();assert(state==MENU&&!endpoint_client.enabled&&!udp_open);
    active_profile=0;start_wifi();wifi_link=ip_link=CYW43_LINK_UP;check_wifi_pm();start_server();
    assert(state==SERVER&&!udp_open&&pm_calls==3&&pm_reads==3&&joins==3);
    assert(erases==0); // No profile/timeout/credential persistence was changed by discovery.
    puts("PASS production app EEX1 glue: HOME direct, late app, authenticated-target cache, TCP reuse, DHCP invalidation, failed rediscovery, PM retained");
}
