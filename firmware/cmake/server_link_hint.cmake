option(PM_SERVER_LINK_HINT "Accept current-session server MAC hints on direct HOME links" OFF)
if(PM_SERVER_LINK_HINT AND PM_HOME_STATIC_ARP_DIAGNOSTIC)
  message(FATAL_ERROR "Server link hints and hardcoded HOME static-ARP diagnostic cannot coexist")
endif()
