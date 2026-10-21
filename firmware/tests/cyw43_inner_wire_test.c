#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wreturn-type"
#define main original_USB_cases
#include "usb_control_test.c"
#undef main
#pragma GCC diagnostic pop
#include <stdlib.h>
#include "../src/cyw43_inner_probe.c"
const char pm_cyw43_inner_translation_unit[]="cyw43-inner-v2:7f4c9375";
void pm_net_cyw_status(pm_cyw_status*out){pm_cyw_probe_status(out);}
bool pm_net_cyw_read(uint64_t b,uint64_t r,unsigned i,pm_cyw_row*out){return pm_cyw_probe_row(b,r,i,out);}
static void fixture_command(const char*s){input(s);input("\n");drain();}
int main(void){
 reset(4);pm_tx_tuple target={.local={192,0,0,1},.peer={192,0,0,2},.local_port=50000,.peer_port=16153};pm_cyw_probe_sync(1,10,2,true,false,UINT64_MAX,0,&target);
 uint8_t buf[128]={0};buf[14]=0x20;uint8_t*p=buf+18;p[12]=8;p[14]=0x45;p[17]=50;p[18]=0x12;p[19]=0x34;p[23]=6;p[26]=192;p[29]=1;p[30]=192;p[33]=2;p[34]=0xc3;p[35]=0x50;p[36]=0x3f;p[37]=0x19;p[41]=1;p[46]=0x50;p[47]=0x18;
 for(unsigned i=0;i<2;i++){
  pm_cyw_probe_copied(64,64);uint32_t id=pm_cyw_probe_open(2,70,buf,sizeof buf,0,1,0);
  if(i){pm_cyw_probe_wait(id);pm_cyw_probe_read(64,-5);p[41]++;pm_cyw_probe_poll(id,-5,0,buf,sizeof buf);}
  pm_cyw_probe_bus(id,buf,sizeof buf,0,1,1);pm_cyw_probe_finish(id,i?-5:0,false);
 }
 pm_cyw_probe_sync(1,10,2,false,false,0,0,&target);
 fixture_command("CY1 CAPS");fixture_command("CY1 STATUS");fixture_command("CY1 QUALITY");
 for(unsigned i=0;i<2;i++)for(unsigned part=0;part<7;part++){char cmd[80];snprintf(cmd,sizeof cmd,"CY1 READ 1 a %x %x",i,part);fixture_command(cmd);}
 fixture_command("CY1 STATUS");fixture_command("CY1 QUALITY");
 assert(!strstr(output,"error="));const char*path=getenv("PM_CYW_EXPORT_FIXTURE");if(path){FILE*f=fopen(path,"wb");assert(f);assert(fwrite(output,1,used,f)==used);assert(!fclose(f));}
 // Stress maximum-width protocol values; all response lines remain <144 bytes.
 probe.boot=probe.run=UINT64_MAX;probe.generation=probe.dropped=probe.incomplete=probe.saturated=probe.depth_lost=probe.rejected=probe.skipped=probe.crc_calls=probe.crc_bytes=probe.crc_us=probe.crc_max_us=UINT32_MAX;
 pm_cyw_row*r=&probe.rows[0];memset(r,0xff,sizeof *r);r->result=INT32_MIN;r->rx_result=INT16_MIN;used=0;output[0]=0;
 fixture_command("CY1 STATUS");fixture_command("CY1 QUALITY");
 for(unsigned part=0;part<7;part++){char cmd[96];snprintf(cmd,sizeof cmd,"CY1 READ ffffffffffffffff ffffffffffffffff 0 %x",part);fixture_command(cmd);}
 assert(!strstr(output,"error="));
 puts("PASS real CY1 USB export:7parts,digest,normal/error rows and144-byte maximum-width line bounds");
}
