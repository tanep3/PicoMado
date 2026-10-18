#include "endpoint_policy.h"

bool pm_endpoint_ipv4_unicast(uint32_t ipv4) {
    uint32_t first=ipv4>>24;
    return first>0 && first<224 && first!=127;
}
static bool link_ready(const pm_endpoint_snapshot *n) {
    return n->ipv4_ready && n->wifi_generation!=0 && pm_endpoint_ipv4_unicast(n->local_ipv4);
}
pm_endpoint_selection pm_endpoint_select(const pm_endpoint_snapshot *n,
        const pm_endpoint_policy *p, const pm_endpoint_cache *cache) {
    pm_endpoint_selection out={.status=PM_ENDPOINT_WAIT_IP,.source=p->mode,
        .wifi_generation=n->wifi_generation,.policy_revision=p->revision};
    if(!link_ready(n))return out;
    switch(p->mode) {
    case PM_ENDPOINT_FIXED:
        out.endpoint=(pm_endpoint){p->fixed_ipv4,p->default_port};break;
    case PM_ENDPOINT_GATEWAY:
        if(!n->gateway_ipv4){out.status=PM_ENDPOINT_WAIT_GATEWAY;return out;}
        out.endpoint=(pm_endpoint){n->gateway_ipv4,p->default_port};break;
    case PM_ENDPOINT_EXCHANGE:
        if(!cache->valid || cache->request.wifi_generation!=n->wifi_generation ||
                cache->request.policy_revision!=p->revision) {
            out.status=PM_ENDPOINT_WAIT_OFFER;return out;
        }
        out.endpoint=cache->endpoint;break;
    default:out.status=PM_ENDPOINT_INVALID_MODE;return out;
    }
    out.status=!pm_endpoint_ipv4_unicast(out.endpoint.ipv4)?PM_ENDPOINT_INVALID_ADDRESS:
        !out.endpoint.port?PM_ENDPOINT_INVALID_PORT:PM_ENDPOINT_READY;
    return out;
}
bool pm_endpoint_accept_offer(const pm_endpoint_snapshot *n,
        const pm_endpoint_policy *p, const pm_endpoint_request *current,
        const pm_endpoint_request *reply, pm_endpoint offered, bool accepting,
        pm_endpoint_cache *next) {
    if(!accepting || p->mode!=PM_ENDPOINT_EXCHANGE || !link_ready(n) ||
            current->wifi_generation!=n->wifi_generation || current->policy_revision!=p->revision ||
            !current->request_generation || reply->wifi_generation!=current->wifi_generation ||
            reply->policy_revision!=current->policy_revision ||
            reply->request_generation!=current->request_generation ||
            !pm_endpoint_ipv4_unicast(offered.ipv4) || !offered.port)return false;
    *next=(pm_endpoint_cache){.valid=true,.endpoint=offered,.request=*reply};
    return true;
}
bool pm_endpoint_attempt_current(pm_endpoint_attempt result, pm_endpoint_attempt current) {
    return result.attempt && result.wifi_generation &&
        result.attempt==current.attempt && result.wifi_generation==current.wifi_generation &&
        result.policy_revision==current.policy_revision;
}
