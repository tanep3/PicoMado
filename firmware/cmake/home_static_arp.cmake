option(PM_HOME_STATIC_ARP_DIAGNOSTIC "Temporary E-ink_1 HOME static-ARP comparison" OFF)
if(PM_HOME_STATIC_ARP_DIAGNOSTIC AND NOT PM_DEVICE STREQUAL "E-ink_1")
  message(FATAL_ERROR "PM_HOME_STATIC_ARP_DIAGNOSTIC is restricted to E-ink_1")
endif()
