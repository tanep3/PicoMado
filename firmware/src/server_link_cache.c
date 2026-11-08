/* BT-AUDIO-20261107: one-sector remembered server-link aggregate and codec. */
#include "server_link_cache.h"
#include <string.h>

enum { SLOT_SIZE=512, SLOT_COUNT=8, RECORD_SIZE=48, FORMAT_VERSION=1 };
typedef struct {
    pm_server_link_cache_record records[2];
    bool present[2];
    uint64_t sequence;
} snapshot;
static struct {
    pm_server_link_cache_io io;
    snapshot pending;
    bool initialized, dirty;
} cache;

static int index_for(pm_profile_id profile) {
    return profile==PM_PROFILE_HOME ? 0 : profile==PM_PROFILE_HOME2 ? 1 : -1;
}
static uint32_t read32(const uint8_t *p) {
    return (uint32_t)p[0]|(uint32_t)p[1]<<8|(uint32_t)p[2]<<16|(uint32_t)p[3]<<24;
}
static void write32(uint8_t *p,uint32_t v) {
    for(unsigned i=0;i<4;i++)p[i]=(uint8_t)(v>>(8*i));
}
static uint64_t read64(const uint8_t *p) {
    return (uint64_t)read32(p)|((uint64_t)read32(p+4)<<32);
}
static void write64(uint8_t *p,uint64_t v) {
    write32(p,(uint32_t)v);write32(p+4,(uint32_t)(v>>32));
}
static uint32_t crc32(const uint8_t *p,size_t n) {
    uint32_t c=UINT32_MAX;
    for(size_t i=0;i<n;i++) {
        c^=p[i];
        for(unsigned j=0;j<8;j++)c=(c>>1)^((0u-(c&1u))&0xedb88320u);
    }
    return ~c;
}
static bool filled(const uint8_t *p,size_t n,uint8_t byte) {
    for(size_t i=0;i<n;i++)if(p[i]!=byte)return false;
    return true;
}
bool pm_server_link_cache_record_valid(const pm_server_link_cache_record *r) {
    if(!r||index_for(r->profile)<0||!r->ssid_length||r->ssid_length>32||
       !r->prefix_length||r->prefix_length>30)return false;
    unsigned first=r->server_ipv4>>24;
    if(!first||first>=224||first==127)return false;
    uint32_t hostmask=UINT32_MAX>>r->prefix_length,host=r->server_ipv4&hostmask;
    if(!host||host==hostmask||(r->mac[0]&1))return false;
    return !filled(r->mac,6,0);
}
static bool record_equal(const pm_server_link_cache_record *a,const pm_server_link_cache_record *b) {
    return a->profile==b->profile&&a->ssid_length==b->ssid_length&&
        !memcmp(a->ssid,b->ssid,a->ssid_length)&&a->server_ipv4==b->server_ipv4&&
        a->prefix_length==b->prefix_length&&!memcmp(a->mac,b->mac,6);
}
static bool contents_equal(const snapshot *a,const snapshot *b) {
    for(unsigned i=0;i<2;i++)if(a->present[i]!=b->present[i]||
        (a->present[i]&&!record_equal(&a->records[i],&b->records[i])))return false;
    return true;
}
static void encode_record(uint8_t *p,const pm_server_link_cache_record *r) {
    memset(p,0,RECORD_SIZE);p[0]=1;p[1]=(uint8_t)r->profile;
    p[2]=r->ssid_length;p[3]=r->prefix_length;write32(p+4,r->server_ipv4);
    memcpy(p+8,r->mac,6);memcpy(p+14,r->ssid,r->ssid_length);
}
static void encode(const snapshot *s,uint8_t page[256],uint8_t marker[256]) {
    memset(page,0xff,256);memcpy(page,"SLCH",4);page[4]=FORMAT_VERSION;page[5]=0;
    page[6]=112;page[7]=0;write64(page+8,s->sequence);
    for(unsigned i=0;i<2;i++) {
        uint8_t *p=page+16+i*RECORD_SIZE;
        if(s->present[i])encode_record(p,&s->records[i]);else memset(p,0,RECORD_SIZE);
    }
    uint32_t crc=crc32(page,252);write32(page+252,crc);
    memset(marker,0xff,256);memcpy(marker,"SLCM",4);marker[4]=FORMAT_VERSION;marker[5]=0;
    marker[6]=24;marker[7]=0;write64(marker+8,s->sequence);
    write32(marker+16,crc);write32(marker+20,~crc);
}
static bool decode(const uint8_t page[256],const uint8_t marker[256],snapshot *out) {
    if(memcmp(page,"SLCH",4)||page[4]!=FORMAT_VERSION||page[5]||page[6]!=112||page[7]||
       !read64(page+8)||read32(page+252)!=crc32(page,252)||!filled(page+112,140,0xff))return false;
    snapshot s={0};s.sequence=read64(page+8);
    for(unsigned i=0;i<2;i++) {
        const uint8_t *p=page+16+i*RECORD_SIZE;
        if(!p[0]) {if(!filled(p,RECORD_SIZE,0))return false;continue;}
        if(p[0]!=1||p[1]!=(i?PM_PROFILE_HOME2:PM_PROFILE_HOME)||p[2]>32)return false;
        pm_server_link_cache_record *r=&s.records[i];r->profile=(pm_profile_id)p[1];
        r->ssid_length=p[2];r->prefix_length=p[3];r->server_ipv4=read32(p+4);
        memcpy(r->mac,p+8,6);memcpy(r->ssid,p+14,r->ssid_length);
        if(!pm_server_link_cache_record_valid(r)||!filled(p+14+r->ssid_length,34-r->ssid_length,0))return false;
        s.present[i]=true;
    }
    uint8_t expected[256],expected_marker[256];encode(&s,expected,expected_marker);
    if(memcmp(page,expected,256)||memcmp(marker,expected_marker,256))return false;
    *out=s;return true;
}
/* Rescan after any IO failure: an SDK timeout may occur after the effect. Never
 * program a partially used slot, nor fill a hole before a later occupied slot. */
static pm_server_link_cache_result scan(snapshot *latest,unsigned *next) {
    memset(latest,0,sizeof(*latest));*next=0;
    uint8_t pair[SLOT_SIZE];bool found=false;
    for(unsigned i=0;i<SLOT_COUNT;i++) {
        pm_server_link_cache_result rc=cache.io.read(cache.io.context,i*SLOT_SIZE,pair,sizeof(pair));
        if(rc!=PM_SERVER_LINK_CACHE_OK)return rc;
        if(!filled(pair,sizeof(pair),0xff))*next=i+1;
        snapshot candidate;
        if(!decode(pair,pair+256,&candidate))continue;
        if(found&&candidate.sequence<=latest->sequence)return PM_SERVER_LINK_CACHE_INVALID;
        *latest=candidate;found=true;
    }
    return found?PM_SERVER_LINK_CACHE_OK:PM_SERVER_LINK_CACHE_EMPTY;
}
pm_server_link_cache_result pm_server_link_cache_load(const pm_server_link_cache_io *io) {
    memset(&cache,0,sizeof(cache));
    if(!io||!io->read||!io->erase||!io->program)return PM_SERVER_LINK_CACHE_INVALID;
    cache.io=*io;cache.initialized=true;snapshot loaded;unsigned next;
    pm_server_link_cache_result rc=scan(&loaded,&next);
    if(rc==PM_SERVER_LINK_CACHE_OK)cache.pending=loaded;
    return rc;
}
bool pm_server_link_cache_get(pm_profile_id profile,const uint8_t *ssid,size_t length,
                              uint32_t server,pm_server_link_cache_record *out) {
    int i=index_for(profile);
    if(i<0||!cache.initialized||!ssid||!length||length>32||!out||!cache.pending.present[i])return false;
    const pm_server_link_cache_record *r=&cache.pending.records[i];
    if(r->ssid_length!=length||r->server_ipv4!=server||memcmp(r->ssid,ssid,length))return false;
    *out=*r;return true;
}
pm_server_link_cache_result pm_server_link_cache_remember(const pm_server_link_cache_record *r) {
    if(!cache.initialized||!pm_server_link_cache_record_valid(r))return PM_SERVER_LINK_CACHE_INVALID;
    int i=index_for(r->profile);
    if(cache.pending.present[i]&&record_equal(&cache.pending.records[i],r))return PM_SERVER_LINK_CACHE_UNCHANGED;
    cache.pending.records[i]=*r;cache.pending.present[i]=true;cache.dirty=true;
    return PM_SERVER_LINK_CACHE_CHANGED;
}
bool pm_server_link_cache_dirty(void) {return cache.dirty;}
bool pm_server_link_cache_needs_erase(void) {
    if(!cache.initialized||!cache.dirty)return false;
    snapshot disk;unsigned next;pm_server_link_cache_result rc=scan(&disk,&next);
    if(rc==PM_SERVER_LINK_CACHE_OK&&contents_equal(&disk,&cache.pending))return false;
    return (rc!=PM_SERVER_LINK_CACHE_OK&&rc!=PM_SERVER_LINK_CACHE_EMPTY)||next>=SLOT_COUNT;
}
pm_server_link_cache_result pm_server_link_cache_flush(bool allow_flash,bool allow_erase) {
    if(!cache.initialized)return PM_SERVER_LINK_CACHE_INVALID;
    if(!cache.dirty)return PM_SERVER_LINK_CACHE_UNCHANGED;
    if(!allow_flash)return PM_SERVER_LINK_CACHE_DEFERRED;
    snapshot disk;unsigned next;pm_server_link_cache_result rc=scan(&disk,&next);
    if(rc!=PM_SERVER_LINK_CACHE_OK&&rc!=PM_SERVER_LINK_CACHE_EMPTY)return rc;
    if(rc==PM_SERVER_LINK_CACHE_OK&&contents_equal(&disk,&cache.pending)) {
        cache.pending.sequence=disk.sequence;cache.dirty=false;return PM_SERVER_LINK_CACHE_OK;
    }
    uint64_t sequence=cache.pending.sequence>disk.sequence?cache.pending.sequence:disk.sequence;
    if(sequence==UINT64_MAX)return PM_SERVER_LINK_CACHE_SEQUENCE_EXHAUSTED;
    if(next>=SLOT_COUNT) {
        if(!allow_erase)return PM_SERVER_LINK_CACHE_NEEDS_ERASE;
        rc=cache.io.erase(cache.io.context);if(rc!=PM_SERVER_LINK_CACHE_OK)return rc;
        uint8_t readback[256];
        for(unsigned offset=0;offset<PM_SERVER_LINK_CACHE_SECTOR_SIZE;offset+=256) {
            rc=cache.io.read(cache.io.context,offset,readback,sizeof(readback));
            if(rc!=PM_SERVER_LINK_CACHE_OK)return rc;
            if(!filled(readback,sizeof(readback),0xff))return PM_SERVER_LINK_CACHE_READBACK_FAILED;
        }
        next=0;
    }
    snapshot writing=cache.pending;writing.sequence=sequence+1;
    uint8_t page[256],marker[256],readback[256];encode(&writing,page,marker);
    uint32_t offset=next*SLOT_SIZE;
    rc=cache.io.program(cache.io.context,offset,page);if(rc!=PM_SERVER_LINK_CACHE_OK)return rc;
    rc=cache.io.read(cache.io.context,offset,readback,256);if(rc!=PM_SERVER_LINK_CACHE_OK)return rc;
    if(memcmp(page,readback,256))return PM_SERVER_LINK_CACHE_READBACK_FAILED;
    rc=cache.io.program(cache.io.context,offset+256,marker);if(rc!=PM_SERVER_LINK_CACHE_OK)return rc;
    rc=cache.io.read(cache.io.context,offset+256,readback,256);if(rc!=PM_SERVER_LINK_CACHE_OK)return rc;
    if(memcmp(marker,readback,256))return PM_SERVER_LINK_CACHE_COMMIT_FAILED;
    cache.pending.sequence=writing.sequence;cache.dirty=false;return PM_SERVER_LINK_CACHE_OK;
}
const char *pm_server_link_cache_result_name(pm_server_link_cache_result r) {
    switch(r) {
#define NAME(x,n) case PM_SERVER_LINK_CACHE_##x:return n
    NAME(OK,"ok");NAME(EMPTY,"empty");NAME(UNCHANGED,"unchanged");NAME(CHANGED,"changed");
    NAME(INVALID,"invalid");NAME(DEFERRED,"deferred");NAME(NEEDS_ERASE,"needs-erase");
    NAME(SEQUENCE_EXHAUSTED,"sequence-exhausted");NAME(READ_FAILED,"read-failed");
    NAME(SAFETY_FAILED,"safety-failed");NAME(ERASE_FAILED,"erase-failed");
    NAME(PROGRAM_FAILED,"program-failed");NAME(READBACK_FAILED,"readback-failed");NAME(COMMIT_FAILED,"commit-failed");
#undef NAME
    default:return "unknown";
    }
}
