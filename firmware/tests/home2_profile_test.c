// Production settings IO and app transitions, with only flash/radio IO mocked.
#define PM_APP_TEST_ENTRY unused_base_timing_test
#include "app_timing_test.c"
static void fixture(uint32_t profile){
    defaults();settings.profile=profile;settings.incarnation=0x123456789abcdef0ull;
    settings.revision=99;settings.wifi_s=25;settings.app_s=18;settings.retry_s=8;
    settings.retry_max_s=60;settings.heartbeat_s=30;settings.offline_s=90;settings.long_ms=1900;
    store_settings();
}
int main(void){
    _Static_assert(PM_PROFILE_HOME==0 && PM_PROFILE_AWAY==1 && PM_PROFILE_HOME2==2,"stable IDs");
    _Static_assert(offsetof(typeof(settings),crc)==52 && sizeof settings==56,"legacy flash layout");
    assert(!strcmp(pm_profile_name(0),"HOME")&&!strcmp(pm_profile_name(1),"AWAY")&&!strcmp(pm_profile_name(2),"HOME2"));
    assert(!profile_ready(PM_PROFILE_COUNT)&&!profile_ready(255));
    bool configured=profile_ready(PM_PROFILE_HOME2);
    assert(configured==(PM_HOME2_SSID[0]!=0 && PM_HOME2_PASSWORD[0]!=0 && PM_HOME2_SERVER_IP[0]!=0));
    for(unsigned id=0;id<PM_PROFILE_COUNT;id++){
        fixture(id);typeof(settings) expected=settings;
        expected.profile=id==PM_PROFILE_HOME2&&!configured?PM_PROFILE_HOME:id;
        unsigned before=erases;memset(&settings,0,sizeof settings);load_settings();
        assert(erases==before&&!memcmp(&settings,&expected,sizeof settings));
    }
    // Unknown IDs and corrupt CRC preserve the existing reset policy.
    fixture(3);load_settings();assert(settings.profile==0&&settings.revision==0&&settings.wifi_s==15);
    fixture(2);test_flash_sector[16]^=1;load_settings();assert(settings.revision==0&&settings.wifi_s==15);
    fixture(1);settings.profile=0;store_settings();typeof(settings) compatible=settings;load_settings();
    assert(settings.profile<=1&&!memcmp(&settings,&compatible,sizeof settings));
    // All ready masks, both directions, wrap and no-candidate behavior.
    const uint8_t order[]={0,2,1};
    for(unsigned mask=0;mask<8;mask++)for(unsigned pos=0;pos<3;pos++)for(unsigned forward=0;forward<2;forward++){
        unsigned expected=order[pos];
        for(unsigned n=1;n<=3;n++){
            unsigned candidate=order[(pos+(forward?n:3-n))%3];
            if(mask&(1u<<candidate)){expected=candidate;break;}
        }
        assert(pm_profile_step(order[pos],mask,forward)==expected);
    }
    printf("PASS HOME2 settings: configured=%u; legacy 0/1, profile2, fallback, full record preservation, CRC, rollback, all navigation masks\n",configured);
}
