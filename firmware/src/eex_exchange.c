#include "eex_exchange.h"
#include "endpoint_policy.h"
#include <string.h>
static uint64_t earlier(uint64_t a,uint64_t b){return a<b?a:b;}
static bool peer_equal(pm_eex_peer a,pm_eex_peer b){return a.ipv4==b.ipv4&&a.port==b.port;}
static bool identity(const pm_eex_exchange *s,const pm_eex_message *m){
    return !strcmp(s->offer.request,m->request)&&!strcmp(s->offer.client,m->client);
}
bool pm_eex_begin(pm_eex_exchange *s,const char *request,const char *client,
        uint64_t generation,uint64_t now,uint32_t wait){
    if(!generation||wait<1000||wait>300000)return false;
    pm_eex_exchange next={.phase=PM_EEX_DISCOVERING,.wifi_generation=generation,
        .discovery_deadline=now+wait,.next_send=now,.discovery_interval=500};
    next.offer.type=PM_EEX_D;
    size_t r=strlen(request),c=strlen(client);
    if(r!=32||!c||c>32)return false;
    memcpy(next.offer.request,request,r+1);memcpy(next.offer.client,client,c+1);
    char wire[128];
    if(!pm_eex_encode(&next.offer,wire,sizeof wire))return false;
    *s=next;return true;
}
bool pm_eex_receive(pm_eex_exchange *s,const pm_eex_message *m,pm_eex_peer from,uint64_t now){
    if(!identity(s,m)||!pm_endpoint_ipv4_unicast(from.ipv4)||!from.port)return false;
    if(m->type==PM_EEX_O){
        uint32_t host=m->source_host?from.ipv4:m->host;
        if(!pm_endpoint_ipv4_unicast(host)||!m->port||!m->ttl||m->ttl>30)return false;
        if(s->phase==PM_EEX_DISCOVERING){
            if(now>=s->discovery_deadline)return false;
            s->offer=*m;s->reporter=from;s->offer_deadline=now+(uint64_t)m->ttl*1000;
            s->phase_deadline=earlier(now+1500,s->offer_deadline);s->next_send=now;
            s->phase=PM_EEX_RECEIPT_WAIT;return true;
        }
        if(now>=s->offer_deadline||!peer_equal(from,s->reporter)||
            strcmp(m->offer,s->offer.offer)||strcmp(m->generation,s->offer.generation)||
            host!=(s->offer.source_host?s->reporter.ipv4:s->offer.host)||m->port!=s->offer.port)return false;
        // Duplicate O never restarts a probe, receipt window or local expiry.
        s->offer_deadline=earlier(s->offer_deadline,now+(uint64_t)m->ttl*1000);
        return true;
    }
    if(m->type!=PM_EEX_K||now>=s->offer_deadline||!peer_equal(from,s->reporter)||
        strcmp(m->offer,s->offer.offer))return false;
    if(s->phase==PM_EEX_RECEIPT_WAIT&&m->ack=='A'&&m->attempt==0){
        s->receipt_confirmed=true;s->phase=PM_EEX_PROBE_PENDING;return true;
    }
    if(s->phase==PM_EEX_RESULT_WAIT&&m->ack=='R'&&m->attempt==s->attempt){
        s->result_confirmed=true;s->phase=PM_EEX_DONE;return true;
    }
    return false;
}
pm_eex_action pm_eex_tick(pm_eex_exchange *s,uint64_t now,unsigned jitter){
    pm_eex_action a={0};
    if(s->phase==PM_EEX_DISCOVERING){
        if(now>=s->discovery_deadline){s->phase=PM_EEX_EXPIRED;return a;}
        if(now<s->next_send)return a;
        a.send=true;a.message=s->offer;a.message.type=PM_EEX_D;
        if(jitter<850||jitter>1150)jitter=1000;
        uint32_t delay=(uint32_t)((uint64_t)s->discovery_interval*jitter/1000);
        if(delay>5000)delay=5000;
        s->next_send=now+delay;
        s->discovery_interval=s->discovery_interval<2942?s->discovery_interval*17/10:5000;
        return a;
    }
    if(s->phase==PM_EEX_RECEIPT_WAIT){
        if(now>=s->offer_deadline){s->phase=PM_EEX_EXPIRED;return a;}
        if(now>=s->phase_deadline)s->phase=PM_EEX_PROBE_PENDING;
        else if(now>=s->next_send){
            a.send=true;a.message=s->offer;a.message.type=PM_EEX_A;s->next_send=now+300;return a;
        }
    }
    if(s->phase==PM_EEX_PROBE_PENDING){
        if(now>=s->offer_deadline){s->phase=PM_EEX_EXPIRED;return a;}
        s->attempt=1;s->phase=PM_EEX_PROBING;a.start_probe=true;return a;
    }
    // A started probe is allowed to finish after offer expiry; reporting is not.
    if(s->phase==PM_EEX_RESULT_WAIT){
        if(now>=s->phase_deadline||now>=s->offer_deadline){s->phase=PM_EEX_DONE;return a;}
        if(now>=s->next_send){
            a.send=true;a.message=s->offer;a.message.type=PM_EEX_R;
            a.message.attempt=s->attempt;a.message.status=s->status;a.message.stage=s->stage;
            s->next_send=now+300;return a;
        }
    }
    return a;
}
bool pm_eex_probe_result(pm_eex_exchange *s,uint64_t generation,unsigned attempt,
        pm_eex_status status,pm_eex_stage stage,uint64_t now){
    if(s->phase!=PM_EEX_PROBING||generation!=s->wifi_generation||attempt!=s->attempt||
        !pm_eex_result_valid(status,stage))return false;
    s->status=status;s->stage=stage;
    s->verified=status==PM_EEX_OK&&stage==PM_EEX_TARGET_CONNECTED;
    s->phase=now>=s->offer_deadline?PM_EEX_DONE:PM_EEX_RESULT_WAIT;
    s->phase_deadline=earlier(now+1500,s->offer_deadline);s->next_send=now;return true;
}
