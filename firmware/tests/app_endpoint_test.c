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
static pm_net_failure failure_kind=PM_NET_FAIL_TCP;
pm_net_failure pm_net_last_failure(void){return failure_kind;}
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

    // Retention begins through the same EOF entry tested with real pixels in
    // fin_recovery_test; exercise HOME and AWAY connection/status glue here.
    state=ACTIVE;retry_with_image(true);last_lcd_one=last_lcd_two=NULL;
    start_server();assert(state==SERVER&&retained_pixels&&!last_lcd_one);
    state=SESSION;process(2,"{\"type\":\"welcome\",\"data\":{\"incarnation\":1,\"session_epoch\":99}}");
    assert(state==ACTIVE&&retained_pixels&&!last_lcd_one);
    active_profile=1;unsigned pm_before=pm_calls,read_before=pm_reads;
    start_wifi();wifi_link=ip_link=CYW43_LINK_UP;check_wifi_pm();start_server();
    assert(state==ENDPOINT&&udp_open&&retained_pixels&&!last_lcd_one);
    assert(pm_calls==pm_before+1&&pm_reads==read_before+1);
    app_offer();poll_endpoint();
    udp_incoming=endpoint_client.exchange.offer;udp_incoming.type=PM_EEX_K;udp_incoming.ack='A';udp_incoming.attempt=0;udp_packet=true;
    clock_ms++;poll_endpoint();assert(state==SERVER&&retained_pixels&&!last_lcd_one);
    state=SESSION;process(2,"{\"type\":\"welcome\",\"data\":{\"incarnation\":1,\"session_epoch\":100}}");
    process(11,"{\"type\":\"error\",\"data\":{\"code\":\"unavailable\"}}");
    assert(state==RETRY&&!retained_pixels&&last_lcd_one);
    assert(erases==0);puts("PASS retention: HOME setup/welcome, AWAY discovery/target, PM and hard protocol-error release");
    // EEX observes DHCP changes before the main transport poll. It must use
    // the same EOF/hard-error policy before resetting the old connection.
    for(unsigned path=0;path<3;path++){
        state=ACTIVE;last_lcd_one=last_lcd_two=NULL;retained_pixels=path!=2;have_display=path==2;
        incarnation=1;session_epoch=100;display_epoch=1;
        last_rendered=(pm_token){incarnation,session_epoch,display_epoch,owner_epoch,99};
        network_stage=path==2?PM_NET_DRAINING:PM_NET_FAILED;
        failure_kind=path==0?PM_NET_FAIL_EOF_TIMEOUT:PM_NET_FAIL_PROTOCOL;
        observed.gateway^=1;clock_ms+=300;poll_endpoint();
        assert(state==RETRY&&retained_pixels==(path!=1));
        assert((last_lcd_one==NULL)==(path!=1));
    }
    assert(erases==0);puts("PASS EEX network-change before poll: EOF timeout/current pixels retained, hard error released");
    puts("PASS production app EEX1 glue: HOME direct, late app, authenticated-target cache, TCP reuse, DHCP invalidation, failed rediscovery, PM retained");
}
