/* Reconfirm current ownership, never adopt a stale completion or unknown link. */
bool pm_bt_runtime_refresh_connected(uint64_t gen,const uint8_t address[6],uint32_t received_ms){
 if(!gen||!address||!ready||stopping||scanning||!wanted||!cid||retry_pending||
    recovery.phase!=PM_BR_OFF||!encoder||!rate||memcmp(peer,address,6)||
    (uint32_t)(btstack_run_loop_get_time_ms()-received_ms)>=25000)return false;
 avdtp_connection_t *c=avdtp_get_connection_for_avdtp_cid(cid);
 if(!c||avdtp_get_connection_for_bd_addr(peer)!=c||
    c->state!=AVDTP_SIGNALING_CONNECTION_OPENED||
    c->a2dp_source_config_process.state!=A2DP_STREAMING_OPENED||
    !c->a2dp_source_config_process.stream_endpoint_configured)return false;
 hci_connection_t *link=hci_connection_for_handle(c->con_handle);
 if(!link||link->state!=OPEN)return false;
 avdtp_stream_endpoint_t *e=c->a2dp_source_config_process.local_stream_endpoint;
 if(!e||c->active_stream_endpoint!=e||!e->l2cap_media_cid||
    (e->state!=AVDTP_STREAM_ENDPOINT_OPENED&&e->state!=AVDTP_STREAM_ENDPOINT_STREAMING))return false;
 if(audio.active){
  if(!pm_bt_rebuffer_refresh(&rebuffer,generation,gen,audio.generation))return false;
 }
 generation=gen;emit((pm_bt_event){.kind=PM_BT_CONNECTED,.generation=gen});return true;
}
