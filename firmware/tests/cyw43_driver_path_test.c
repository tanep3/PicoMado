/* Exact SDK send + SPI poll + SDPCM parser, hardware IO/async callback mocked. */
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#include <inttypes.h>
#include "cyw43_inner_probe.h"
#if PM_CYW43_INNER_PROBE
#include "../src/cyw43_inner_probe.c"
#endif
#define CYW43_USE_SPI 1
#define CYW43_CLEAR_SDIO_INT 0
#define F1_OVERFLOW_CHANGE 0
#define CYW43_USE_STATS 0
#define CYW43_EINVAL 22
#define CYW43_ETIMEDOUT 110
#define CYW43_FAIL_FAST_CHECK(x) (x)
#define CYW43_DEBUG(...) ((void)0)
#define CYW43_VDEBUG(...) ((void)0)
#define CYW43_WARN(...) ((void)0)
#define CYW43_STAT_INC(...) ((void)0)
#define CYW43_SDPCM_SEND_COMMON_WAIT ((void)0)
#define CYW43_WRITE_BYTES_PAD(n) (((n)+3u)&~3u)
#define WLAN_FUNCTION 2
#define BUS_FUNCTION 0
#define BACKPLANE_FUNCTION 1
#define SPI_INTERRUPT_REGISTER 1
#define SPI_STATUS_REGISTER 2
#define SPI_FRAME_CONTROL 3
#define BUS_OVERFLOW_UNDERFLOW 4
#define F2_PACKET_AVAILABLE 1
#define GSPI_PACKET_AVAILABLE 1
#define F2_F3_FIFO_RD_UNDERFLOW 2
#define CONTROL_HEADER 0
#define ASYNCEVENT_HEADER 1
#define DATA_HEADER 2
#define BDC_HEADER_LEN 4
#define SDPCM_HEADER_LEN 12
#define IOCTL_HEADER_LEN 16
#define CDCF_IOC_ID_MASK 0xffff0000u
#define CDCF_IOC_ID_SHIFT 16
#define CYW43_ERROR_WRONG_PAYLOAD_TYPE -9
struct sdpcm_header_t {uint16_t size,size_com;uint8_t sequence,channel_and_flags,next_length,header_length,wireless_flow_control,bus_data_credit,reserved[2];};
struct sdpcm_bdc_header_t {uint8_t flags,priority,flags2,data_offset;};
struct ioctl_header_t {uint32_t cmd,len,flags;int32_t status;};
typedef struct {uint8_t wlan_flow_control,wwd_sdpcm_last_bus_data_credit,wwd_sdpcm_packet_transmit_sequence_number;uint16_t wwd_sdpcm_requested_ioctl_id;bool had_successful_packet;void *cb_data;_Alignas(4) uint8_t spid_buf[2048];} cyw43_int_t;
typedef cyw43_int_t cyw43_ll_t;
#define CYW_INT_FROM_LL(s) (s)
struct pbuf {const uint8_t *payload;unsigned len,tot_len,ref;struct pbuf *next;};
static struct {uint8_t data[256];unsigned n,partial;int error;} fixture_rx[3];
static unsigned fixture_rx_count,fixture_rx_next,fixture_reads,fixture_writes,fixture_callbacks,fixture_nested;
static int fixture_write_error;static uint32_t fixture_ticks,fixture_trace;
static uint64_t fixture_probe_us;
static uint8_t fixture_packet[512],fixture_written[4096];static unsigned fixture_written_n;
static cyw43_int_t *fixture_self;
static int cyw43_ll_sdpcm_poll_device(cyw43_int_t *,size_t *,uint8_t **);
static int cyw43_ll_send_ethernet(cyw43_ll_t *,int,size_t,const void *,bool);
static int cyw43_sdpcm_send_common(cyw43_int_t *,uint32_t,size_t,uint8_t *);
static void fixture_hash(uint32_t v){for(unsigned i=0;i<4;i++){fixture_trace=(fixture_trace^(uint8_t)v)*16777619u;v>>=8;}}
static uint32_t fixture_hash_bytes(const uint8_t*p,unsigned n){uint32_t h=2166136261u;for(unsigned i=0;i<n;i++)h=(h^p[i])*16777619u;return h;}
uint64_t time_us_64(void){fixture_probe_us+=7;return fixture_probe_us;}
static uint32_t cyw43_hal_ticks_us(void){fixture_ticks+=100001;return fixture_ticks;}
static void cyw43_ll_bus_sleep(void*s,bool sleep){assert(s==fixture_self&&!sleep);fixture_hash(10);}
static int cyw43_cb_read_host_interrupt_pin(void*ctx){assert(ctx==fixture_self);fixture_hash(11);return fixture_rx_next<fixture_rx_count;}
static uint16_t cyw43_read_reg_u16(cyw43_int_t*s,int fn,int at){assert(s==fixture_self&&fn==BUS_FUNCTION&&at==SPI_INTERRUPT_REGISTER);fixture_hash(12);return fixture_rx_next<fixture_rx_count?F2_PACKET_AVAILABLE:0;}
static uint32_t cyw43_read_reg_u32(cyw43_int_t*s,int fn,int at){assert(s==fixture_self&&fn==BUS_FUNCTION&&at==SPI_STATUS_REGISTER);fixture_hash(13);return fixture_rx_next<fixture_rx_count?(fixture_rx[fixture_rx_next].n<<9)|GSPI_PACKET_AVAILABLE:0;}
static void cyw43_write_reg_u16(cyw43_int_t*s,int fn,int at,uint16_t v){assert(s==fixture_self&&fn==BUS_FUNCTION&&at==SPI_INTERRUPT_REGISTER);fixture_hash(14);fixture_hash(v);}
static void cyw43_write_reg_u8(cyw43_int_t*s,int fn,int at,uint8_t v){assert(s==fixture_self&&fn==BACKPLANE_FUNCTION&&at==SPI_FRAME_CONTROL);fixture_hash(15);fixture_hash(v);}
static int cyw43_read_bytes(cyw43_int_t*s,int fn,int at,size_t n,uint8_t*out){assert(s==fixture_self&&fn==WLAN_FUNCTION&&at==0&&out==s->spid_buf&&fixture_rx_next<fixture_rx_count);unsigned slot=fixture_rx_next++;assert(n==fixture_rx[slot].n);unsigned copy=fixture_rx[slot].partial?fixture_rx[slot].partial:(unsigned)n;memcpy(out,fixture_rx[slot].data,copy);fixture_reads++;fixture_hash(16);fixture_hash((uint32_t)n);fixture_hash((uint32_t)fixture_rx[slot].error);return fixture_rx[slot].error;}
static int cyw43_write_bytes(cyw43_int_t*s,int fn,int at,size_t n,uint8_t*p){assert(s==fixture_self&&fn==WLAN_FUNCTION&&at==0&&p==s->spid_buf);assert(fixture_written_n+n<sizeof fixture_written);memcpy(fixture_written+fixture_written_n,p,n);fixture_written_n+=(unsigned)n;fixture_writes++;fixture_hash(17);fixture_hash((uint32_t)n);fixture_hash(fixture_hash_bytes(p,(unsigned)n));fixture_hash((uint32_t)fixture_write_error);return fixture_write_error;}
static unsigned pbuf_copy_partial(const struct pbuf*p,void*out,unsigned n,unsigned off){unsigned copied=0;assert(n+off<=p->tot_len);while(p&&copied<n){if(off>=p->len){off-=p->len;p=p->next;continue;}unsigned take=p->len-off;if(take>n-copied)take=n-copied;memcpy((uint8_t*)out+copied,p->payload+off,take);copied+=take;off=0;p=p->next;}fixture_hash(18);fixture_hash(n);return copied;}
static void *cyw43_ll_parse_async_event(size_t n,uint8_t*p){assert(n==40);return p;}
static void cyw43_cb_process_async_event(cyw43_int_t*s,void*event){(void)event;assert(s==fixture_self);fixture_callbacks++;fixture_hash(19);if(fixture_nested){unsigned nesting=fixture_nested;fixture_nested=0;if(nesting==1){memset(s->spid_buf+12,0x55,16);assert(cyw43_sdpcm_send_common(s,CONTROL_HEADER,16,s->spid_buf)==0);}else{uint8_t packet[64];memcpy(packet,fixture_packet,64);packet[41]=77;struct pbuf p={packet,64,64,1,NULL};assert(cyw43_ll_send_ethernet(s,0,64,&p,true)==0);}}}
#include "generated_sdk_functions.inc"
static void fixture_word(uint8_t*p,unsigned at,unsigned v){p[at]=v>>8;p[at+1]=v;}
static void fixture_frame(unsigned n){memset(fixture_packet,0,sizeof fixture_packet);uint8_t*p=fixture_packet;p[12]=8;p[14]=0x45;fixture_word(p,16,n-14);fixture_word(p,18,0x1234);p[23]=6;p[26]=192;p[29]=1;p[30]=192;p[33]=2;fixture_word(p,34,50000);fixture_word(p,36,16153);p[38]=p[39]=p[40]=0xff;p[41]=0xf8;p[46]=0x50;p[47]=0x18;for(unsigned i=54;i<n;i++)p[i]=(uint8_t)i;}
static void fixture_rx_frame(unsigned slot,unsigned kind,unsigned credit){
 unsigned n=kind==3?12:kind==CONTROL_HEADER?44:82;memset(fixture_rx[slot].data,0,sizeof fixture_rx[slot].data);fixture_rx[slot].n=n;
 struct sdpcm_header_t*h=(void*)fixture_rx[slot].data;h->size=(uint16_t)n;h->size_com=(uint16_t)~n;h->channel_and_flags=kind==3?CONTROL_HEADER:kind;h->bus_data_credit=(uint8_t)credit;h->header_length=kind==CONTROL_HEADER?12:14;
 if(kind==DATA_HEADER){fixture_rx[slot].data[14]=0x20;memcpy(fixture_rx[slot].data+18,fixture_packet,64);fixture_word(fixture_rx[slot].data,18+18,999);fixture_rx[slot].data[18+41]=33;}
 if(kind==ASYNCEVENT_HEADER){fixture_rx[slot].data[14]=0x20;uint8_t*p=fixture_rx[slot].data+18;p[12]=0x88;p[13]=0x6c;p[19]=0;p[20]=0x10;p[21]=0x18;}
}
int main(void){
 for(unsigned scenario=0;scenario<18;scenario++){
  cyw43_int_t state={0};fixture_self=&state;state.cb_data=&state;memset(state.spid_buf,0xcc,sizeof state.spid_buf);fixture_probe_us=100;fixture_ticks=fixture_trace=fixture_reads=fixture_writes=fixture_callbacks=fixture_nested=fixture_rx_count=fixture_rx_next=fixture_written_n=0;fixture_write_error=0;memset(fixture_rx,0,sizeof fixture_rx);fixture_frame(scenario==11?400:64);
  bool stall=(scenario>=1&&scenario<=7)||scenario==9||scenario==10||scenario==15||scenario==16;state.wwd_sdpcm_last_bus_data_credit=stall?0:200;
  if(scenario==16){fixture_rx_frame(0,DATA_HEADER,1);fixture_rx[0].data[16]=1;fixture_rx_count=1;}
  if(scenario==15){state.wwd_sdpcm_last_bus_data_credit=255;state.wwd_sdpcm_packet_transmit_sequence_number=255;fixture_rx_frame(0,3,0);fixture_rx_count=1;}
  if(scenario==1){fixture_rx_frame(0,3,1);fixture_rx_count=1;}
  if(scenario>=2&&scenario<=4){fixture_rx_frame(0,scenario==2?DATA_HEADER:scenario==3?CONTROL_HEADER:ASYNCEVENT_HEADER,1);fixture_rx_count=1;}
  if(scenario==5||scenario==6){fixture_rx_frame(0,DATA_HEADER,1);if(scenario==5)fixture_rx[0].data[2]^=1;else{fixture_rx[0].partial=40;fixture_rx[0].error=-5;}fixture_rx_frame(1,3,1);fixture_rx_count=2;}
  if(scenario==8)fixture_write_error=-5;
  if(scenario==9||scenario==10){fixture_rx_frame(0,ASYNCEVENT_HEADER,1);fixture_rx_frame(1,3,2);fixture_rx_count=2;fixture_nested=scenario==9?1:2;}
#if PM_CYW43_INNER_PROBE
  memset(&probe,0,sizeof probe);pm_tx_tuple target={.local={192,0,0,1},.peer={192,0,0,2},.local_port=50000,.peer_port=16153};pm_cyw_probe_sync(1,scenario+1,2,true,false,1000000000,0,&target);
#endif
  unsigned length=scenario==11?400:64;struct pbuf tail={fixture_packet+45,length-45,length-45,2,NULL};struct pbuf head={fixture_packet,45,length,3,&tail};struct pbuf before_head=head,before_tail=tail;uint8_t saved[512];memcpy(saved,fixture_packet,sizeof saved);
  if(scenario==17){tail.len-=5;before_tail=tail;}
  int result=cyw43_ll_send_ethernet(&state,scenario==14?1:0,scenario==12?2040:length,&head,true);
  assert(!memcmp(&head,&before_head,sizeof head)&&!memcmp(&tail,&before_tail,sizeof tail)&&!memcmp(saved,fixture_packet,sizeof saved));
  if(scenario==7)assert(result==-110&&!fixture_writes);else if(scenario==12)assert(result==-22&&!fixture_writes);else if(scenario==8)assert(result==-5&&fixture_writes==1);else assert(result==0&&fixture_writes==(scenario==9||scenario==10?2u:1u));
  if(scenario==13){fixture_packet[19]++;assert(cyw43_ll_send_ethernet(&state,0,length,&head,true)==0);assert(fixture_writes==2);}
  printf("wire scenario=%u result=%d reads=%u writes=%u callback=%u trace=%08x bytes=%u data=%08x credit=%u sequence=%u\n",scenario,result,fixture_reads,fixture_writes,fixture_callbacks,fixture_trace,fixture_written_n,fixture_hash_bytes(fixture_written,fixture_written_n),state.wwd_sdpcm_last_bus_data_credit,state.wwd_sdpcm_packet_transmit_sequence_number);
#if PM_CYW43_INNER_PROBE
  assert(!probe.depth);
  if(scenario==12||scenario==17){assert(probe.rejected==1&&!probe.count);if(scenario==17)assert(probe.incomplete&&!probe.crc_calls);continue;}
  if(scenario==14){assert(probe.skipped==1&&!probe.count);continue;}
  assert(probe.count==(scenario==10||scenario==13?2u:1u));pm_cyw_row*r=&probe.rows[0];assert(r->flags&PM_CY_DONE);assert(r->original.ipid==0x1234&&r->original.seq==0xfffffff8u);
  if(scenario==0||scenario==1||scenario==8||scenario==11||scenario==13||scenario==15)assert(!(r->flags&PM_CY_CHANGED));
  if((scenario>=2&&scenario<=6)||scenario==9||scenario==10||scenario==16)assert(r->flags&PM_CY_CHANGED);
  if(scenario==5){assert(r->flags&PM_CY_RX_CRC);assert(r->rx_result==-1&&!r->rx_length&&r->io_calls==2);}
  if(scenario==6)assert(r->flags&PM_CY_IO_ERROR);
  if(scenario==7)assert((r->flags&PM_CY_TIMEOUT)&&!(r->flags&PM_CY_BUS)&&probe.crc_calls==1);
  if(scenario==9||scenario==10)assert(r->nested==1&&r->depth==1);
  if(scenario==10)assert(probe.rows[1].depth==2&&probe.rows[1].original.seq!=r->original.seq);
  if(scenario==11)assert(r->flags&PM_CY_PREFIX);
  if(scenario==16)assert(r->rx_length==0x8040&&!probe.saturated);
  if(scenario==15)assert(r->sequence_before==255&&r->sequence_bus==0&&r->credit_before==255&&r->credit_bus==0);
  if(scenario==13)assert(probe.rows[1].original.seq==r->original.seq&&probe.rows[1].original.ipid!=r->original.ipid&&probe.rows[1].ticket!=r->ticket);
  assert(probe.crc_calls<=3*probe.count&&probe.crc_bytes<=3*256*probe.count);
  printf("diagnostic scenario=%u rows=%u flags=%x polls=%u rx=%u/%u/%u bad=%u io=%u/%u crc_calls=%u\n",scenario,probe.count,r->flags,r->polls,r->rx_data,r->rx_control,r->rx_event,r->rx_bad,r->io_calls,r->io_errors,probe.crc_calls);
#endif
 }
 puts("PASS exact SDK TX/SPI poll/RX parser: ON/OFF bus trace,normal/flow/DATA/CONTROL/ASYNC/invalid/partial-error/timeout/bus-error/nested/prefix/length/retransmission/interface; unchanged pbuf");
}
