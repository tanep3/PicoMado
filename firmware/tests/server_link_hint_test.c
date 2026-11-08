#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "server_link_hint.h"
#define GOOD_HINT "{\"version\":1,\"server_ipv4\":\"192.168.0.120\",\"client_ipv4\":\"192.168.0.44\",\"prefix_length\":24,\"mac\":\"2C:cf:67:11:72:f3\"}"
static const char good[]="{\"type\":\"welcome\",\"data\":{\"incarnation\":3,\"session_epoch\":100,\"media_input_version\":1,\"server_link\":" GOOD_HINT "}}";
static bool parse(const char *s,pm_server_welcome *w){return pm_server_welcome_parse(s,strlen(s),w);}
static void optional(const char *hint,pm_server_link_parse expected){
    char s[1025];snprintf(s,sizeof s,"{\"type\":\"welcome\",\"data\":{\"incarnation\":3,\"session_epoch\":100,\"server_link\":%s}}",hint);
    pm_server_welcome w;assert(parse(s,&w)&&w.link==expected&&w.incarnation==3&&w.session_epoch==100);
}
int main(void){
    pm_server_welcome w;assert(parse(good,&w)&&w.link==PM_SERVER_LINK_VALID&&w.media_input_version==1);
    assert(w.hint.server_ipv4==0xc0a80078u&&w.hint.client_ipv4==0xc0a8002cu&&w.hint.mac[0]==0x2c&&w.hint.mac[5]==0xf3);
    const char *old="{\"type\":\"welcome\",\"data\":{\"incarnation\":3,\"session_epoch\":100}}";
    assert(parse(old,&w)&&w.link==PM_SERVER_LINK_ABSENT);
    optional("null",PM_SERVER_LINK_ABSENT);optional("{}",PM_SERVER_LINK_INVALID);optional("[]",PM_SERVER_LINK_INVALID);optional("5",PM_SERVER_LINK_INVALID);optional("true",PM_SERVER_LINK_INVALID);
    const char *bad[]={
      "{\"type\":\"welcome\",\"data\":{\"\\u0069ncarnation\":3,\"session_epoch\":100}}",
      "{\"type\":\"welcome\",\"data\":{\"incarnation\":3,\"incarnation\":4,\"session_epoch\":100}}",
      "{\"type\":\"welcome\",\"data\":{\"incarnation\":18446744073709551616,\"session_epoch\":100}}",
      "{\"type\":\"welcome\",\"data\":{\"incarnation\":-3,\"session_epoch\":100}}",
      "{\"type\":\"welcome\",\"data\":{\"incarnation\":3,\"session_epoch\":0}}",
      "{\"type\":\"welcome\",\"data\":{\"incarnation\":1e2,\"session_epoch\":100}}",
      "{\"type\":\"welcome\",\"data\":{\"incarnation\":3,\"session_epoch\":100},\"data\":{}}",
      "{\"type\":\"wrong\",\"unknown\":{\"type\":\"welcome\"},\"data\":{\"incarnation\":3,\"session_epoch\":100}}",
      "{\"type\":\"welcome\",\"data\":{\"incarnation\":3,\"session_epoch\":100,\"unknown\":[[[[[[[[0]]]]]]]]}}"
    };
    for(unsigned i=0;i<sizeof bad/sizeof *bad;i++)assert(!parse(bad[i],&w));
    assert(parse("{\"unknown\":{\"incarnation\":999,\"session_epoch\":888},\"type\":\"welcome\",\"data\":{\"incarnation\":3,\"session_epoch\":100}}",&w)&&w.incarnation==3&&w.session_epoch==100);
    assert(parse("{\"type\":\"welcome\",\"data\":{\"incarnation\":3,\"session_epoch\":100,\"server_link\":" GOOD_HINT ",\"server_link\":null,\"server_link\":" GOOD_HINT "}}",&w)&&w.link==PM_SERVER_LINK_INVALID);
    const char *from[]={"\"version\":1","192.168.0.120","192.168.0.44","\"prefix_length\":24","2C:cf:67:11:72:f3","2C:cf:67:11:72:f3","2C:cf:67:11:72:f3","2C:cf:67:11:72:f3"};
    const char *to[]={"\"version\":2","192.168.0.999","192.168.00.44","\"prefix_length\":31","00:00:00:00:00:00","ff:ff:ff:ff:ff:ff","01:00:00:00:00:01","02:cf:67:11:72:f3"};
    for(unsigned i=0;i<8;i++){
        char changed[1025];const char *at=strstr(GOOD_HINT,from[i]);assert(at);
        snprintf(changed,sizeof changed,"%.*s%s%s",(int)(at-GOOD_HINT),GOOD_HINT,to[i],at+strlen(from[i]));
        optional(changed,i==7?PM_SERVER_LINK_VALID:PM_SERVER_LINK_INVALID);
    }
    for(size_t n=0;n<strlen(good);n++)assert(!pm_server_welcome_parse(good,n,&w));
    char large[1026];memset(large,' ',sizeof large);memcpy(large,old,strlen(old));assert(!pm_server_welcome_parse(large,sizeof large,&w));
    // Bounded malformed mutations exercise the real parser under ASan/UBSan.
    unsigned random=1;for(unsigned n=0;n<5000;n++){
        char changed[sizeof good];memcpy(changed,good,sizeof good);random=random*1664525u+1013904223u;
        size_t at=random%(sizeof good-1);changed[at]=(char)(random>>24);(void)pm_server_welcome_parse(changed,sizeof good-1,&w);
    }
    assert(parse(good,&w));
    pm_server_link_scope q={.connection=1,.generation=2,.netif_identity=3,.local_ipv4=0xc0a8002c,.peer_ipv4=0xc0a80078,.mask=0xffffff00,.profile=PM_PROFILE_HOME,.ready=true,.up=true,.direct=true};
    assert(pm_server_link_admit(&q,&q,&w.hint));pm_server_link_scope s=q;s.profile=PM_PROFILE_HOME2;assert(pm_server_link_admit(&s,&s,&w.hint));
    for(unsigned i=0;i<10;i++){
        s=q;switch(i){case 0:s.connection++;break;case 1:s.generation++;break;case 2:s.netif_identity++;break;case 3:s.local_ipv4++;break;case 4:s.peer_ipv4++;break;case 5:s.mask=0xffff0000;break;case 6:s.profile=PM_PROFILE_AWAY;break;case 7:s.ready=false;break;case 8:s.up=false;break;case 9:s.direct=false;break;}
        assert(!pm_server_link_admit(&q,&s,&w.hint));
    }
    s=q;s.profile=PM_PROFILE_AWAY;assert(!pm_server_link_admit(&s,&s,&w.hint));
    assert(!pm_server_link_prefix(0xffff00ff)&&!pm_server_link_prefix(0)&&!pm_server_link_prefix(UINT32_MAX));
    pm_server_link_hint h=w.hint;h.mac[0]=2;assert(pm_server_link_admit(&q,&q,&h));h.mac[0]=1;assert(!pm_server_link_admit(&q,&q,&h));
    h=w.hint;h.prefix_length=25;assert(!pm_server_link_admit(&q,&q,&h));h=w.hint;h.client_ipv4++;assert(!pm_server_link_admit(&q,&q,&h));
    puts("PASS actual Welcome parser/domain: optional compatibility, nested/escaped/duplicate keys, triple hint, overflow, all-byte cutoff, depth/size, mutations and scope fences");
}
