#include "endpoint_policy.h"
#include <assert.h>
#include <stdio.h>

#define IP(a,b,c,d) (((uint32_t)(a)<<24)|((uint32_t)(b)<<16)|((uint32_t)(c)<<8)|(d))
int main(void) {
    pm_endpoint_snapshot n={true,IP(10,1,2,3),IP(10,1,2,1),7};
    pm_endpoint_policy p={PM_ENDPOINT_FIXED,IP(192,0,2,1),48115,4};
    pm_endpoint_cache c={0};
    pm_endpoint_selection s=pm_endpoint_select(&n,&p,&c);
    assert(s.status==PM_ENDPOINT_READY && s.endpoint.ipv4==p.fixed_ipv4 && s.endpoint.port==48115);
    n.ipv4_ready=false;assert(pm_endpoint_select(&n,&p,&c).status==PM_ENDPOINT_WAIT_IP);n.ipv4_ready=true;
    n.wifi_generation=0;assert(pm_endpoint_select(&n,&p,&c).status==PM_ENDPOINT_WAIT_IP);n.wifi_generation=7;
    p.default_port=0;assert(pm_endpoint_select(&n,&p,&c).status==PM_ENDPOINT_INVALID_PORT);p.default_port=48115;
    p.fixed_ipv4=IP(127,0,0,1);assert(pm_endpoint_select(&n,&p,&c).status==PM_ENDPOINT_INVALID_ADDRESS);
    assert(!pm_endpoint_ipv4_unicast(0) && !pm_endpoint_ipv4_unicast(IP(224,0,0,1)) &&
        !pm_endpoint_ipv4_unicast(IP(255,255,255,255)));
    assert(pm_endpoint_ipv4_unicast(IP(192,0,2,0))); // .0 is not necessarily a subnet address.
    p.mode=PM_ENDPOINT_GATEWAY;s=pm_endpoint_select(&n,&p,&c);
    assert(s.status==PM_ENDPOINT_READY && s.endpoint.ipv4==n.gateway_ipv4);
    n.gateway_ipv4=0;assert(pm_endpoint_select(&n,&p,&c).status==PM_ENDPOINT_WAIT_GATEWAY);
    n.gateway_ipv4=IP(10,9,8,1);assert(pm_endpoint_select(&n,&p,&c).endpoint.ipv4==IP(10,9,8,1));
    p.mode=PM_ENDPOINT_EXCHANGE;assert(pm_endpoint_select(&n,&p,&c).status==PM_ENDPOINT_WAIT_OFFER);
    pm_endpoint_request req={7,4,10},reply=req;
    pm_endpoint offered={IP(10,9,8,1),48115};
    assert(pm_endpoint_accept_offer(&n,&p,&req,&reply,offered,true,&c));
    assert(pm_endpoint_select(&n,&p,&c).status==PM_ENDPOINT_READY);
    // App exit/TCP retry do not mutate network generation or erase the observation.
    pm_endpoint_attempt a={7,4,1},b={7,4,2};
    assert(!pm_endpoint_attempt_current(a,b) && pm_endpoint_attempt_current(b,b));
    assert(pm_endpoint_select(&n,&p,&c).endpoint.ipv4==offered.ipv4);
    req.request_generation++; // A new discovery request does not erase a usable cache.
    assert(pm_endpoint_select(&n,&p,&c).status==PM_ENDPOINT_READY);
    assert(!pm_endpoint_accept_offer(&n,&p,&req,&reply,offered,true,&c));
    reply=req;assert(!pm_endpoint_accept_offer(&n,&p,&req,&reply,offered,false,&c));
    pm_endpoint bad={0,48115};assert(!pm_endpoint_accept_offer(&n,&p,&req,&reply,bad,true,&c));
    bad=(pm_endpoint){offered.ipv4,0};assert(!pm_endpoint_accept_offer(&n,&p,&req,&reply,bad,true,&c));
    assert(c.endpoint.ipv4==offered.ipv4 && c.request.request_generation==10); // Rejection preserved state.
    n.wifi_generation=8;assert(pm_endpoint_select(&n,&p,&c).status==PM_ENDPOINT_WAIT_OFFER);
    assert(!pm_endpoint_accept_offer(&n,&p,&req,&reply,offered,true,&c));
    req.wifi_generation=8;reply=req;
    assert(pm_endpoint_accept_offer(&n,&p,&req,&reply,offered,true,&c));
    p.revision=5;assert(pm_endpoint_select(&n,&p,&c).status==PM_ENDPOINT_WAIT_OFFER);
    assert(!pm_endpoint_accept_offer(&n,&p,&req,&reply,offered,true,&c));
    assert(!pm_endpoint_attempt_current(b,(pm_endpoint_attempt){8,4,2}));
    assert(!pm_endpoint_attempt_current(b,(pm_endpoint_attempt){7,5,2}));
    assert(!pm_endpoint_attempt_current((pm_endpoint_attempt){0},(pm_endpoint_attempt){0}));
    p.mode=PM_ENDPOINT_FIXED;p.fixed_ipv4=IP(192,0,2,5);
    assert(!pm_endpoint_accept_offer(&n,&p,&req,&reply,offered,true,&c));
    assert(pm_endpoint_select(&n,&p,&c).endpoint.ipv4==IP(192,0,2,5));
    p.mode=(pm_endpoint_mode)99;assert(pm_endpoint_select(&n,&p,&c).status==PM_ENDPOINT_INVALID_MODE);
    puts("PASS endpoint policy: fixed/gateway/exchange, WiFi lifetime, stale request/attempt, preserved settings");
}
