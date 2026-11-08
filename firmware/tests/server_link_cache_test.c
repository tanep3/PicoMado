#include "server_link_cache.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

enum { PREFIX=1024, SIZE=PREFIX+4096+12288 };
static uint8_t media[SIZE],before[SIZE];
static unsigned reads,programs,erases;
static int byte_budget=-1,fail_read=-1,fail_program=-1;
static bool fail_erase,fail_safety,corrupt_data,corrupt_marker,corrupt_erase;
static pm_server_link_cache_record home={.profile=PM_PROFILE_HOME,.ssid_length=4,.ssid={'H','o',0,'e'},
    .server_ipv4=0xc0a80102,.prefix_length=24,.mac={2,0x10,0x20,0x30,0x40,0x50}};
static pm_server_link_cache_io io;
static bool reenter_enabled,inside_reentry,expected_dirty;
static unsigned reentry_checks;
static void reenter(void);

static pm_server_link_cache_result read_io(void *ctx,uint32_t at,uint8_t *out,size_t n) {
    (void)ctx;assert(at<=4096&&n<=4096-at);reenter();
    if((int)++reads==fail_read)return PM_SERVER_LINK_CACHE_READ_FAILED;
    memcpy(out,media+PREFIX+at,n);
    /* Callback recursion must not overwrite the scan/readback destination. */
    uint8_t saved[512];assert(n<=sizeof(saved));memcpy(saved,out,n);reenter();
    assert(!memcmp(saved,out,n));return PM_SERVER_LINK_CACHE_OK;
}
static pm_server_link_cache_result apply(uint32_t at,const uint8_t *p,size_t n,bool erase) {
    for(size_t j=0;j<n;j++) {
        if(byte_budget==0)return erase?PM_SERVER_LINK_CACHE_ERASE_FAILED:PM_SERVER_LINK_CACHE_PROGRAM_FAILED;
        if(byte_budget>0)--byte_budget;
        if(erase)media[PREFIX+at+j]=0xff;
        else {assert((media[PREFIX+at+j]&p[j])==p[j]);media[PREFIX+at+j]&=p[j];}
    }
    return PM_SERVER_LINK_CACHE_OK;
}
static pm_server_link_cache_result erase_io(void *ctx) {
    (void)ctx;reenter();++erases;if(fail_safety)return PM_SERVER_LINK_CACHE_SAFETY_FAILED;
    if(fail_erase)return PM_SERVER_LINK_CACHE_ERASE_FAILED;
    pm_server_link_cache_result rc=apply(0,NULL,4096,true);
    if(corrupt_erase)media[PREFIX+1024]=0;
    reenter();return rc;
}
static pm_server_link_cache_result program_io(void *ctx,uint32_t at,const uint8_t *p) {
    (void)ctx;assert(at%256==0&&at<=3840);
    uint8_t saved[256];memcpy(saved,p,sizeof(saved));reenter();
    assert(!memcmp(saved,p,sizeof(saved)));++programs;
    if(fail_safety)return PM_SERVER_LINK_CACHE_SAFETY_FAILED;
    if((int)programs==fail_program)return PM_SERVER_LINK_CACHE_PROGRAM_FAILED;
    pm_server_link_cache_result rc=apply(at,p,256,false);
    if(corrupt_data&&at%512==0)media[PREFIX+at]^=1;
    if(corrupt_marker&&at%512==256)media[PREFIX+at]^=1;
    reenter();assert(!memcmp(saved,p,sizeof(saved)));return rc;
}
/* Distinct IO table catches attempted replacement of the outer operation's
 * binding even when re-entered load returns before a recursive read. */
static pm_server_link_cache_result trap_read(void *ctx,uint32_t at,uint8_t *p,size_t n) {
    (void)ctx;(void)at;(void)p;(void)n;assert(!"re-entry replaced the IO binding");
    return PM_SERVER_LINK_CACHE_READ_FAILED;
}
static void reenter(void) {
    if(!reenter_enabled)return;
    assert(!inside_reentry);inside_reentry=true;++reentry_checks;
    unsigned r=reads,p=programs,e=erases;
    pm_server_link_cache_io other=io;other.read=trap_read;
    pm_server_link_cache_record replacement=home,out,untouched;
    replacement.mac[5]^=0x7f;
    memset(&out,0xa7,sizeof(out));memcpy(&untouched,&out,sizeof(out));
    assert(pm_server_link_cache_dirty()==expected_dirty);
    assert(pm_server_link_cache_load(&other)==PM_SERVER_LINK_CACHE_DEFERRED);
    assert(pm_server_link_cache_load(NULL)==PM_SERVER_LINK_CACHE_DEFERRED);
    assert(pm_server_link_cache_remember(&replacement)==PM_SERVER_LINK_CACHE_DEFERRED);
    replacement.profile=PM_PROFILE_HOME2;
    assert(pm_server_link_cache_remember(&replacement)==PM_SERVER_LINK_CACHE_DEFERRED);
    assert(pm_server_link_cache_remember(NULL)==PM_SERVER_LINK_CACHE_DEFERRED);
    assert(pm_server_link_cache_flush(true,true)==PM_SERVER_LINK_CACHE_DEFERRED);
    assert(pm_server_link_cache_flush(false,false)==PM_SERVER_LINK_CACHE_DEFERRED);
    assert(pm_server_link_cache_needs_erase());
    assert(!pm_server_link_cache_get(home.profile,home.ssid,home.ssid_length,home.server_ipv4,&out));
    assert(!memcmp(&out,&untouched,sizeof(out)));
    assert(pm_server_link_cache_record_valid(&replacement));
    assert(!strcmp(pm_server_link_cache_result_name(PM_SERVER_LINK_CACHE_DEFERRED),"deferred"));
    assert(pm_server_link_cache_dirty()==expected_dirty);
    assert(reads==r&&programs==p&&erases==e);inside_reentry=false;
}
static void reset_faults(void) {
    reads=programs=erases=0;byte_budget=fail_read=fail_program=-1;
    reenter_enabled=inside_reentry=expected_dirty=false;reentry_checks=0;
    fail_erase=fail_safety=corrupt_data=corrupt_marker=corrupt_erase=false;
}
static void reset(void) {
    memset(media,0x5a,sizeof(media));memset(media+PREFIX,0xff,4096);reset_faults();
    assert(pm_server_link_cache_load(&io)==PM_SERVER_LINK_CACHE_EMPTY);
}
static bool get(pm_server_link_cache_record *out) {
    return pm_server_link_cache_get(home.profile,home.ssid,home.ssid_length,home.server_ipv4,out);
}
static void preserved(void) {
    assert(!memcmp(media,before,PREFIX));
    assert(!memcmp(media+PREFIX+4096,before+PREFIX+4096,12288));
}
static void commit(pm_server_link_cache_record *r) {
    assert(pm_server_link_cache_remember(r)==PM_SERVER_LINK_CACHE_CHANGED);
    assert(pm_server_link_cache_flush(true,true)==PM_SERVER_LINK_CACHE_OK);
    assert(!pm_server_link_cache_dirty());
}
static uint32_t crc(const uint8_t *p,size_t n) {
    uint32_t c=UINT32_MAX;for(size_t i=0;i<n;i++){c^=p[i];for(unsigned j=0;j<8;j++)c=(c>>1)^((0u-(c&1))&0xedb88320u);}return ~c;
}
static void put32(uint8_t *p,uint32_t v){for(unsigned i=0;i<4;i++)p[i]=(uint8_t)(v>>(8*i));}
static void rechecksum(uint8_t *p) {
    uint32_t c=crc(p,252);put32(p+252,c);memcpy(p+256+8,p+8,8);
    put32(p+256+16,c);put32(p+256+20,~c);
}
static void rules(void) {
    reset();assert(!pm_server_link_cache_dirty());assert(pm_server_link_cache_record_valid(&home));
    pm_server_link_cache_record r=home,out;
    r.profile=PM_PROFILE_AWAY;assert(!pm_server_link_cache_record_valid(&r));r=home;
    for(unsigned p=0;p<256;p++){r.prefix_length=(uint8_t)p;
        bool expected=p>=1&&p<=30&&(r.server_ipv4&(UINT32_MAX>>p))!=0&&(r.server_ipv4&(UINT32_MAX>>p))!=(UINT32_MAX>>p);
        assert(pm_server_link_cache_record_valid(&r)==expected);}
    r=home;for(unsigned n=0;n<=33;n++){r.ssid_length=(uint8_t)n;assert(pm_server_link_cache_record_valid(&r)==(n>0&&n<=32));}
    r=home;memset(r.mac,0,6);assert(!pm_server_link_cache_record_valid(&r));
    memset(r.mac,255,6);assert(!pm_server_link_cache_record_valid(&r));r=home;r.mac[0]=1;assert(!pm_server_link_cache_record_valid(&r));
    const uint32_t bad[]={0,0x00010203,0x7f010203,0xe0010203,0xffffffff,0xc0a80100,0xc0a801ff};
    for(unsigned i=0;i<sizeof(bad)/sizeof(*bad);i++){r=home;r.server_ipv4=bad[i];assert(!pm_server_link_cache_record_valid(&r));}
    r=home;commit(&r);assert(get(&out));assert(out.mac[0]==2);
    assert(!pm_server_link_cache_get(PM_PROFILE_AWAY,home.ssid,4,home.server_ipv4,&out));
    assert(!pm_server_link_cache_get(PM_PROFILE_HOME2,home.ssid,4,home.server_ipv4,&out));
    assert(!pm_server_link_cache_get(home.profile,(const uint8_t *)"Home",4,home.server_ipv4,&out));
    assert(!pm_server_link_cache_get(home.profile,home.ssid,4,home.server_ipv4+1,&out));
    unsigned writes=programs;assert(pm_server_link_cache_remember(&home)==PM_SERVER_LINK_CACHE_UNCHANGED);
    assert(pm_server_link_cache_flush(true,true)==PM_SERVER_LINK_CACHE_UNCHANGED);assert(programs==writes);
    r=home;r.profile=PM_PROFILE_HOME2;r.mac[5]++;commit(&r);
    assert(pm_server_link_cache_load(&io)==PM_SERVER_LINK_CACHE_OK);assert(get(&out));assert(out.mac[5]==home.mac[5]);
    assert(pm_server_link_cache_get(r.profile,r.ssid,4,r.server_ipv4,&out));assert(out.mac[5]==r.mac[5]);
    assert(media[PREFIX+20]==2&&media[PREFIX+21]==1&&media[PREFIX+22]==168&&media[PREFIX+23]==192);
    r=home;r.mac[5]++;assert(pm_server_link_cache_remember(&r)==PM_SERVER_LINK_CACHE_CHANGED);
    assert(pm_server_link_cache_remember(&home)==PM_SERVER_LINK_CACHE_CHANGED);writes=programs;
    assert(pm_server_link_cache_flush(true,true)==PM_SERVER_LINK_CACHE_OK);assert(programs==writes);
    puts("PASS record rules, profile/SSID/target fences, binary SSID, fixed LE encoding, unchanged/coalesced no-write");
}
static void cuts(bool full) {
    reset();pm_server_link_cache_record r=home;commit(&r);
    if(full)for(unsigned i=1;i<8;i++){r.mac[5]++;commit(&r);}
    memcpy(before,media,SIZE);uint8_t old=r.mac[5];r.mac[5]++;
    unsigned limit=full?4608:512;
    for(unsigned cut=0;cut<=limit;cut++) {
        memcpy(media,before,SIZE);reset_faults();assert(pm_server_link_cache_load(&io)==PM_SERVER_LINK_CACHE_OK);
        assert(pm_server_link_cache_remember(&r)==PM_SERVER_LINK_CACHE_CHANGED);
        assert(pm_server_link_cache_needs_erase()==full);byte_budget=(int)cut;
        pm_server_link_cache_result rc=pm_server_link_cache_flush(true,true);
        assert(pm_server_link_cache_dirty()==(rc!=PM_SERVER_LINK_CACHE_OK));preserved();
        reset_faults();rc=pm_server_link_cache_load(&io);
        assert(rc==PM_SERVER_LINK_CACHE_OK||rc==PM_SERVER_LINK_CACHE_EMPTY);
        pm_server_link_cache_record out;
        if(get(&out))assert(out.mac[5]==old||out.mac[5]==r.mac[5]);else assert(full);
        assert(!programs&&!erases);assert(!pm_server_link_cache_dirty());
    }
    printf("PASS %u %s byte-cut boundaries and neighboring settings/BT byte preservation\n",limit+1,full?"rollover":"append");
}
static void invalid_pages(void) {
    reset();commit(&home);memcpy(before,media,SIZE);
    for(unsigned i=0;i<512;i++) {
        memcpy(media,before,SIZE);media[PREFIX+i]^=1;reset_faults();
        assert(pm_server_link_cache_load(&io)==PM_SERVER_LINK_CACHE_EMPTY);assert(!programs&&!erases&&!pm_server_link_cache_dirty());
    }
    /* Recomputed CRC cannot make a wrong schema, profile, shape or sequence valid. */
    const unsigned at[]={4,6,16+1,16+2,16+3,16+46,64,112};
    for(unsigned i=0;i<sizeof(at)/sizeof(*at);i++) {
        memcpy(media,before,SIZE);media[PREFIX+at[i]]^=1;
        if(at[i]==18)media[PREFIX+at[i]]=33;
        if(at[i]==19)media[PREFIX+at[i]]=0;
        rechecksum(media+PREFIX);
        assert(pm_server_link_cache_load(&io)==PM_SERVER_LINK_CACHE_EMPTY);
    }
    memcpy(media,before,SIZE);memcpy(media+PREFIX+64,media+PREFIX+16,48);rechecksum(media+PREFIX);
    assert(pm_server_link_cache_load(&io)==PM_SERVER_LINK_CACHE_EMPTY); /* duplicate HOME in HOME2 slot */
    memcpy(media,before,SIZE);memset(media+PREFIX+8,0xff,8);rechecksum(media+PREFIX);
    assert(pm_server_link_cache_load(&io)==PM_SERVER_LINK_CACHE_OK);
    pm_server_link_cache_record r=home;r.mac[5]++;
    assert(pm_server_link_cache_remember(&r)==PM_SERVER_LINK_CACHE_CHANGED);
    assert(pm_server_link_cache_flush(true,true)==PM_SERVER_LINK_CACHE_SEQUENCE_EXHAUSTED);assert(pm_server_link_cache_dirty());
    memcpy(media,before,SIZE);memcpy(media+PREFIX+512,media+PREFIX,512);
    assert(pm_server_link_cache_load(&io)==PM_SERVER_LINK_CACHE_INVALID);assert(!get(&r));
    memset(media+PREFIX,0x5a,4096);reset_faults();assert(pm_server_link_cache_load(&io)==PM_SERVER_LINK_CACHE_EMPTY);
    assert(!programs&&!erases); /* old unrelated bytes never trigger a defaults write */
    puts("PASS all 512 corrupt bytes, unknown version, shape, duplicate/wrong profile, sequence no-wrap/conflict, old/unformatted media");
}
static void failures(void) {
    for(unsigned mode=0;mode<8;mode++) {
        reset();commit(&home);memcpy(before,media,SIZE);reset_faults();
        pm_server_link_cache_record r=home;r.mac[5]++;
        assert(pm_server_link_cache_remember(&r)==PM_SERVER_LINK_CACHE_CHANGED);
        pm_server_link_cache_result expected;
        switch(mode) {
        case 0:fail_safety=true;expected=PM_SERVER_LINK_CACHE_SAFETY_FAILED;break;
        case 1:fail_program=1;expected=PM_SERVER_LINK_CACHE_PROGRAM_FAILED;break;
        case 2:fail_program=2;expected=PM_SERVER_LINK_CACHE_PROGRAM_FAILED;break;
        case 3:corrupt_data=true;expected=PM_SERVER_LINK_CACHE_READBACK_FAILED;break;
        case 4:corrupt_marker=true;expected=PM_SERVER_LINK_CACHE_COMMIT_FAILED;break;
        case 5:fail_read=1;expected=PM_SERVER_LINK_CACHE_READ_FAILED;break;
        case 6:fail_read=9;expected=PM_SERVER_LINK_CACHE_READ_FAILED;break;
        default:fail_read=10;expected=PM_SERVER_LINK_CACHE_READ_FAILED;break;
        }
        assert(pm_server_link_cache_flush(true,true)==expected);assert(pm_server_link_cache_dirty());
        pm_server_link_cache_record out;assert(get(&out)&&out.mac[5]==r.mac[5]);preserved();
        reset_faults();assert(pm_server_link_cache_flush(true,true)==PM_SERVER_LINK_CACHE_OK);assert(!pm_server_link_cache_dirty());
        if(mode==7)assert(programs==0); /* uncertain committed outcome deduplicates on rescan */
        assert(pm_server_link_cache_load(&io)==PM_SERVER_LINK_CACHE_OK);assert(get(&out)&&out.mac[5]==r.mac[5]);
    }
    reset();pm_server_link_cache_record r=home;
    for(unsigned i=0;i<8;i++){r.mac[5]++;commit(&r);}memcpy(before,media,SIZE);reset_faults();
    r.mac[5]++;assert(pm_server_link_cache_remember(&r)==PM_SERVER_LINK_CACHE_CHANGED);
    assert(pm_server_link_cache_flush(false,true)==PM_SERVER_LINK_CACHE_DEFERRED);
    assert(pm_server_link_cache_flush(true,false)==PM_SERVER_LINK_CACHE_NEEDS_ERASE);assert(!programs&&!erases);
    fail_erase=true;assert(pm_server_link_cache_flush(true,true)==PM_SERVER_LINK_CACHE_ERASE_FAILED);
    assert(pm_server_link_cache_dirty());preserved();
    fail_erase=false;corrupt_erase=true;
    assert(pm_server_link_cache_flush(true,true)==PM_SERVER_LINK_CACHE_READBACK_FAILED);assert(pm_server_link_cache_dirty());preserved();
    reset_faults();assert(pm_server_link_cache_flush(true,true)==PM_SERVER_LINK_CACHE_OK);preserved();
    reset_faults();fail_read=1;assert(pm_server_link_cache_load(&io)==PM_SERVER_LINK_CACHE_READ_FAILED);assert(!get(&r));
    puts("PASS safety/erase/program/read/commit failures preserve dirty RAM; uncertain commit rescan; allow flags; retry");
}
static void reentry(void) {
    static uint8_t reference[SIZE];
    for(unsigned full=0;full<=1;full++) {
        reset();pm_server_link_cache_record r=home,home2=home,out;
        home2.profile=PM_PROFILE_HOME2;home2.ssid[0]='2';home2.mac[5]^=0x40;
        commit(&home2);commit(&r);
        if(full)for(unsigned i=2;i<8;i++){r.mac[5]++;commit(&r);}
        memcpy(before,media,SIZE);r.mac[5]++;
        /* Exact durable-byte reference from the same operation without recursion. */
        commit(&r);memcpy(reference,media,SIZE);memcpy(media,before,SIZE);reset_faults();
        reenter_enabled=true;expected_dirty=false;
        assert(pm_server_link_cache_load(&io)==PM_SERVER_LINK_CACHE_OK);
        assert(reentry_checks==16&&!programs&&!erases&&!pm_server_link_cache_dirty());
        assert(!memcmp(media,before,SIZE));
        assert(pm_server_link_cache_remember(&r)==PM_SERVER_LINK_CACHE_CHANGED);
        expected_dirty=true;reentry_checks=0;
        assert(pm_server_link_cache_needs_erase()==(bool)full);
        assert(reentry_checks==16&&pm_server_link_cache_dirty());
        assert(!memcmp(media,before,SIZE));
        reentry_checks=0;
        assert(pm_server_link_cache_flush(true,true)==PM_SERVER_LINK_CACHE_OK);
        assert(reentry_checks==(full?58u:24u));assert(!pm_server_link_cache_dirty());
        assert(!memcmp(media,reference,SIZE));preserved();
        assert(get(&out)&&out.mac[5]==r.mac[5]);
        assert(pm_server_link_cache_get(home2.profile,home2.ssid,home2.ssid_length,home2.server_ipv4,&out));
        assert(out.mac[5]==home2.mac[5]);
        /* Both RAM records and reloaded durable records survive every callback. */
        expected_dirty=false;
        assert(pm_server_link_cache_load(&io)==PM_SERVER_LINK_CACHE_OK);
        assert(get(&out)&&out.mac[5]==r.mac[5]);
        assert(pm_server_link_cache_get(home2.profile,home2.ssid,home2.ssid_length,home2.server_ipv4,&out));
        assert(out.mac[5]==home2.mac[5]);
        assert(!memcmp(media,reference,SIZE));
    }
    /* The guard must release on load/scan/flush failure, not just successful IO. */
    reset();commit(&home);reset_faults();reenter_enabled=true;fail_read=1;
    assert(pm_server_link_cache_load(&io)==PM_SERVER_LINK_CACHE_READ_FAILED);
    pm_server_link_cache_record r=home,out;r.mac[5]++;
    assert(!get(&out));assert(pm_server_link_cache_remember(&r)==PM_SERVER_LINK_CACHE_CHANGED);
    expected_dirty=true;fail_read=(int)reads+1;
    assert(pm_server_link_cache_needs_erase());assert(get(&out)&&out.mac[5]==r.mac[5]);
    fail_read=-1;fail_program=1;
    assert(pm_server_link_cache_flush(true,true)==PM_SERVER_LINK_CACHE_PROGRAM_FAILED);
    assert(pm_server_link_cache_dirty());assert(get(&out)&&out.mac[5]==r.mac[5]);
    fail_program=-1;assert(pm_server_link_cache_flush(true,true)==PM_SERVER_LINK_CACHE_OK);
    expected_dirty=false;assert(pm_server_link_cache_load(&io)==PM_SERVER_LINK_CACHE_OK);
    assert(get(&out)&&out.mac[5]==r.mac[5]);
    reenter_enabled=false;
    assert(pm_server_link_cache_load(NULL)==PM_SERVER_LINK_CACHE_INVALID);
    assert(pm_server_link_cache_load(&io)==PM_SERVER_LINK_CACHE_OK); /* invalid-load exit released */
    puts("PASS IO callback re-entry across load/needs-erase/append/rollover/failures: guard, scratch/pending/outputs/IO/bytes preserved");
}
int main(void) {
    io=(pm_server_link_cache_io){.read=read_io,.erase=erase_io,.program=program_io};
    assert(pm_server_link_cache_load(NULL)==PM_SERVER_LINK_CACHE_INVALID);
    assert(pm_server_link_cache_remember(&home)==PM_SERVER_LINK_CACHE_INVALID);
    rules();cuts(false);cuts(true);invalid_pages();failures();reentry();
    puts("PASS actual production cache aggregate/codec; RAM flash only, no hardware acceptance");return 0;
}
