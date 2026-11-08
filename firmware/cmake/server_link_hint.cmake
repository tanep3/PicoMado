option(PM_SERVER_LINK_HINT "Accept current-session server MAC hints on direct HOME links" OFF)
if(PM_SERVER_LINK_HINT AND PM_HOME_STATIC_ARP_DIAGNOSTIC)
  message(FATAL_ERROR "Server link hints and hardcoded HOME static-ARP diagnostic cannot coexist")
endif()

option(PM_SERVER_LINK_CACHE "Remember validated HOME server MACs for bounded pre-TCP reuse" OFF)
if(PM_SERVER_LINK_CACHE AND NOT PM_SERVER_LINK_HINT)
  message(FATAL_ERROR "Remembered server MACs require PM_SERVER_LINK_HINT")
endif()
