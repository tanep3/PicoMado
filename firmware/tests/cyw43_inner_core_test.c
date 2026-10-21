#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "../src/cyw43_inner_probe.c"
static uint64_t now;
uint64_t time_us_64(void){return now++;}
static uint8_t buffer[512];
static pm_tx_tuple tuple={.local={192,0,0,1},.peer={192,0,0,2},.local_port=50000,.peer_port=16153};
static void put_word(unsigned at,unsigned v){buffer[at]=v>>8;buffer[at+1]=v;}
static void setup(uint64_t run,unsigned eth_length){
 memset(&probe,0,sizeof probe);memset(buffer,0,sizeof buffer);now=100;buffer[14]=0x20;uint8_t*p=buffer+18;p[12]=8;p[14]=0x45;put_word(18+16,eth_length-14);p[23]=6;p[26]=192;p[29]=1;p[30]=192;p[33]=2;put_word(18+34,50000);put_word(18+36,16153);p[41]=1;p[46]=0x50;p[47]=0x18;
 pm_cyw_probe_sync(1,run,2,true,false,10000000,0,&tuple);
}
static uint32_t open(unsigned n){pm_cyw_probe_copied(n,n);return pm_cyw_probe_open(2,n+6,buffer,sizeof buffer,0,1,0);}
static void done(uint32_t id){pm_cyw_probe_bus(id,buffer,sizeof buffer,0,1,1);pm_cyw_probe_finish(id,0,false);}
int main(void){
 assert(pm_cyw_crc32((const uint8_t*)"123456789",9)==0xcbf43926);
 setup(1,64);uint32_t id=open(64);assert(id&&probe.count==1&&probe.rows[0].domain==68&&probe.rows[0].covered==68);buffer[0]=0xff;buffer[12]=0xa5;buffer[13]=0x5a;done(id);assert(!(probe.rows[0].flags&PM_CY_CHANGED)&&probe.crc_calls==2);
 setup(1,300);id=open(300);assert(probe.rows[0].flags&PM_CY_PREFIX);buffer[14+280]^=1;done(id);assert(!(probe.rows[0].flags&PM_CY_CHANGED)&&probe.crc_bytes==512); // prefix limit is explicit.
 setup(1,64);id=open(64);pm_cyw_probe_wait(id);pm_cyw_probe_read(40,-5);buffer[30]^=1;pm_cyw_probe_poll(id,-5,0,buffer,sizeof buffer);assert(probe.rows[0].flags&PM_CY_RX_CHANGED);assert(probe.rows[0].rx_length==0);done(id);assert(probe.rows[0].io_errors==1&&probe.rows[0].flags&PM_CY_IO_ERROR&&probe.crc_calls==3);
 setup(1,64);for(unsigned i=0;i<PM_CYW_INNER_CAP+1;i++){id=open(64);done(id);}assert(probe.count==PM_CYW_INNER_CAP&&probe.dropped==1);assert(probe.crc_calls==2*PM_CYW_INNER_CAP);
 uint32_t digest_before=digest();pm_cyw_probe_sync(1,1,2,false,false,10000000,now,&tuple);pm_cyw_row r;assert(pm_cyw_probe_row(1,1,0,&r));assert(!pm_cyw_probe_row(2,1,0,&r));pm_cyw_probe_finish(id,9,false);assert(digest()==digest_before); // stale/duplicate finish cannot mutate frozen evidence.
 pm_cyw_probe_sync(1,0,2,false,false,0,0,&tuple);assert(probe.count==PM_CYW_INNER_CAP);pm_cyw_probe_sync(1,2,2,true,false,10000000,0,&tuple);assert(!probe.count);
 setup(1,64);uint32_t ids[5];for(unsigned i=0;i<5;i++)ids[i]=open(64);assert(ids[4]==0&&probe.depth_lost==1);for(unsigned i=4;i;i--)done(ids[i-1]);assert(!probe.depth);assert(probe.rows[0].nested==3);
 setup(1,64);id=open(64);pm_cyw_probe_status(&(pm_cyw_status){0});assert(!pm_cyw_probe_row(1,1,0,&r));pm_cyw_probe_sync(1,2,2,true,false,10000000,0,&tuple);assert(probe.incomplete&&probe.run==1&&!probe.active);done(id);pm_cyw_probe_sync(1,2,2,true,false,10000000,0,&tuple);assert(probe.run==2&&!probe.count);
 setup(1,64);id=open(64);probe.rows[0].polls=UINT32_MAX;pm_cyw_probe_poll(id,-1,0,buffer,sizeof buffer);probe.reads=UINT32_MAX;pm_cyw_probe_read(1,0);done(id);assert(probe.saturated&&probe.rows[0].flags&PM_CY_SAT);
 setup(1,64);id=open(64);now=UINT32_MAX-20u;pm_cyw_probe_wait(id);now+=50;done(id);assert(probe.rows[0].wait_us>=50&&probe.rows[0].wait_us<60);
 setup(1,64);id=open(64);pm_cyw_probe_sync(1,1,3,true,false,10000000,0,&tuple);assert(!probe.active&&probe.changed);done(id);
 setup(1,64);assert(!pm_cyw_probe_open(2,1000,buffer,64,0,0,0) || probe.rejected); // invalid view never reads out of bounds.
 setup(1,64);uint8_t*p=buffer+18;memmove(p+18,p+14,50);put_word(18+12,0x8100);put_word(18+14,0);put_word(18+16,0x0800);id=open(68);done(id);assert(probe.count==1&&probe.rows[0].original.type==0x0800);
 setup(1,64);p=buffer+18;memmove(p+22,p+14,50);put_word(18+12,0x88a8);put_word(18+14,0);put_word(18+16,0x8100);put_word(18+18,0);put_word(18+20,0x0800);id=open(72);done(id);assert(probe.count==1);
 setup(1,64);p=buffer+18;memmove(p+74,p+34,30);p[14]=0x4f;put_word(18+16,90);id=open(104);done(id);assert(probe.count==1&&probe.rows[0].original.length==10);
 setup(1,64);p=buffer+18;memmove(p+66,p+54,10);p[46]=0x80;put_word(18+16,62);id=open(76);done(id);assert(probe.count==1&&probe.rows[0].original.length==10);
 setup(1,64);put_word(18+20,1);id=open(64);pm_cyw_probe_finish(id,0,false);assert(!probe.count&&!probe.crc_calls);
 setup(1,64);put_word(18+12,0x0806);id=open(64);pm_cyw_probe_finish(id,0,false);assert(!probe.count&&!probe.crc_calls);
 setup(1,64);put_word(18+16,200);id=open(64);pm_cyw_probe_finish(id,0,false);assert(!probe.count&&!probe.crc_calls);
 setup(1,64);pm_cyw_probe_copied(64,59);id=pm_cyw_probe_open(2,70,buffer,sizeof buffer,0,1,0);pm_cyw_probe_finish(id,0,false);assert(!probe.count&&probe.rejected&&probe.incomplete&&!probe.crc_calls);
 puts("PASS inner core: CRC domain/prefix, errors,8+1 bound,freeze/retain/reset,partial,nested/depth,stale ticket,saturation,clock wrap,generation,bounds");
}
