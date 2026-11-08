#define main pcm_media_main
#include "bluetooth_media_test.c"
#undef main
static uint8_t sbc_packet[40+8*77];
static void sbc_chunk(uint64_t stream,uint64_t first,unsigned count){
 assert(count<=8);be64(sbc_packet,1);be64(sbc_packet+8,2);be64(sbc_packet+16,3);be64(sbc_packet+24,stream);be64(sbc_packet+32,first);
 process_sbc_bytes(sbc_packet,40+77*count);
}
static void load_fixture(const char *path){
 FILE *f=fopen(path,"r");assert(f);char line[1024],hex[240];unsigned mode,alloc,pool;bool found=false;
 while(fgets(line,sizeof line,f))if(sscanf(line,"%u %u %u %239s",&mode,&alloc,&pool,hex)==4&&mode==3&&alloc==0&&pool==32){
  assert(strlen(hex)==154);for(unsigned i=0;i<77;i++){unsigned n;assert(sscanf(hex+2*i,"%2x",&n)==1);sbc_packet[40+i]=(uint8_t)n;}found=true;break;
 }
 fclose(f);assert(found);for(unsigned i=1;i<8;i++)memcpy(sbc_packet+40+77*i,sbc_packet+40,77);
}
static void sbc_ready(uint64_t first){
 reset();assert(command(28,1,first)==PM_API_PENDING);assert(bt_media.ready_pending&&!plays);
 now_ms=10;poll_audio();assert(last_kind==29&&!bt_media.ready_pending&&!plays);
 assert(strstr(last_report,"\"codec_epoch\":1")&&strstr(last_report,"\"capacity_frames\":16384")&&strstr(last_report,"\"channel_mode\":3"));
}
int main(int argc,char **argv){
 assert(argc==2);pcm_media_main();load_fixture(argv[1]);
 sbc_ready(19);for(unsigned i=0;i<16;i++){sbc_chunk(1,19+1024*i,8);now_ms+=10;poll_audio();assert(plays==(i==15));}
 assert(ring.used==16384&&bt_media.accepted==16403);sbc_chunk(1,19,8);assert(!bt_media.failed&&ring.used==16384);
 sbc_chunk(1,16403,1);assert(bt_media.failed&&!bt_media.active);
 // Wrong generation ignored; a partial stale overlap is a protocol failure.
 sbc_ready(0);sbc_chunk(2,0,1);assert(!ring.used);sbc_chunk(1,0,1);sbc_chunk(1,0,2);assert(bt_media.failed);
 // Invalid final frame is rejected, empty EOF doesn't request AVDTP START.
 sbc_ready(19);assert(command(26,1,20)==PM_API_REJECTED);assert(command(26,1,19)==PM_API_PENDING);now_ms=20;poll_audio();assert(!bt_media.active&&!bt_media.failed&&!plays);
 sbc_ready(19);assert(command(26,1,147)==PM_API_PENDING);sbc_chunk(1,19,1);now_ms=20;poll_audio();assert(plays==1);
 // Malformed tail is atomic and terminal, not a valid first-frame admission.
 sbc_ready(0);sbc_packet[40+77+3]^=1;sbc_chunk(1,0,2);assert(bt_media.failed&&bt_media.accepted==0);sbc_packet[40+77+3]^=1;
 // Current identity with truncated binary header is terminal; stale is ignored.
 for(size_t n=32;n<=40;n++){
  sbc_ready(0);be64(sbc_packet+24,2);process_sbc_bytes(sbc_packet,n);assert(bt_media.active);be64(sbc_packet+24,1);process_sbc_bytes(sbc_packet,n);assert(bt_media.failed);
 }
 sbc_ready(UINT64_MAX-127);sbc_chunk(1,UINT64_MAX-127,1);assert(bt_media.failed&&bt_media.accepted==UINT64_MAX-127);
 // Ready blocks early source data, preserves one bounded retry and cancels on expiry.
 reset();command(28,1,0);sbc_chunk(1,0,1);assert(bt_media.failed&&!bt_media.ready_pending);
 reset();command(28,1,0);send_ok=false;now_ms=10;poll_audio();assert(last_kind==29&&bt_media.blocked&&bt_media.ready_pending);
 now_ms=20;poll_audio();assert(bt_media.blocked_ms==10);send_ok=true;now_ms=30;poll_audio();assert(!bt_media.ready_pending&&last_kind==29);now_ms=40;poll_audio();assert(last_kind==25);
 reset();command(28,1,0);send_ok=false;now_ms=10;poll_audio();now_ms=1510;poll_audio();assert(expired==1&&!bt_media.active&&bt_media.failed&&!bt_media.ready_pending);
 reset();command(28,1,0);send_ok=false;now_ms=10;poll_audio();command(23,1,0);send_ok=true;now_ms=20;poll_audio();assert(last_kind==25&&!bt_media.ready_pending);
 reset();command(28,1,0);display_epoch++;now_ms=10;poll_audio();assert(!bt_media.active&&!reports);
 // Codec/link invalidation before Ready does not publish an old configuration.
 reset();command(28,1,0);ring.active=false;now_ms=10;poll_audio();assert(last_kind==25&&bt_media.failed&&!bt_media.ready_pending);
 puts("SBC wire bridge: Ready tuple/retry/deadline, identity, full prefill, atomic CRC rejection, EOF and cancellation PASS");return 0;
}
