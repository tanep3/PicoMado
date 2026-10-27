# Host-only mapping for the tracked, approved sample hardware profiles.
pm_test_profile_header() {
  case "$1" in
    sample-a|sample-b) printf '%s\n' "$PWD/profiles/pico_w_lcd_160x80_4.h";;
    E-ink_0) printf '%s\n' "$PWD/profiles/pico_w_eink_250x122_0.h";;
    E-ink_1) printf '%s\n' "$PWD/profiles/pico_w_eink_250x122_2.h";;
    *) return 2;;
  esac
}
