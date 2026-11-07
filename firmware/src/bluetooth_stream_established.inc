/* Real adapter completion handler, isolated for fake-SDK host execution. */
static void handle_stream_established(uint16_t c,uint8_t status){
 if(c!=cid)return;
 if(!wanted||recovery_releasing()){
  if(!status)a2dp_source_disconnect(c);
  return;
 }
 if(recovery.phase!=PM_BR_OFF&&(uint32_t)(btstack_run_loop_get_time_ms()-recovery.started)>=25000){
  retry_cancel();recovery_fail(0xff10);if(!status)a2dp_source_disconnect(c);return;
 }
 if(status){retry_cancel();recovery_fail(status);return;}
 retry_cancel();recovery_cancel();
 emit((pm_bt_event){.kind=PM_BT_CONNECTED});
 if(!remote_cid){uint16_t rc=0;if(!avrcp_connect(peer,&rc))remote_cid=rc;}
}
