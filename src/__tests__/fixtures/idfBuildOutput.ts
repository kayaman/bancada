// Verbatim `idf.py build` output, captured on 2026-08-31 on Linux against
// ESP-IDF v6.0.1 (riscv32-esp-elf gcc 15.2.0) building
// `examples/get-started/hello_world` for esp32c3. Paths are scrubbed; nothing
// else is retyped.
//
// Three shape facts here would not have survived guessing, and each one breaks
// something if assumed the other way:
//
//  1. **ESP-IDF writes essentially everything to STDOUT.** The failing build
//     that produced this emitted 166 stdout lines and *one* stderr line — the
//     gcc diagnostic, ninja's `FAILED:` and its `build stopped` trailer are
//     all stdout. arduino-cli puts diagnostics on stderr, so any code that
//     assumes "errors are on stderr" is wrong here.
//  2. **ninja keeps building after the error.** `FAILED:` appears at line 11
//     of 166, and compilation continues for the rest of the log — so the error
//     is nowhere near the tail, and a tail-based excerpt misses it entirely.
//  3. **The progress lines dominate.** A clean build of this trivial example
//     emits ~500 `[n/total]` lines, which is why the console collapses them
//     into a single replaced-in-place row.

import type { OutputLine } from "../../api";

/** A compile error, with the ninja progress and trailer around it. */
export const IDF_BUILD_FAILURE: readonly OutputLine[] = [
  { stream: "stdout", line: "Executing action: all (aliases: build)" },
  { stream: "stdout", line: "Running ninja in directory /tmp/idf-fail/build" },
  { stream: "stdout", line: "Executing \"ninja all\"..." },
  { stream: "stdout", line: "[1/19] Linking C static library esp-idf/esp_bootloader_format/libesp_bootloader_format.a" },
  { stream: "stdout", line: "[2/19] Linking C static library esp-idf/esp_app_format/libesp_app_format.a" },
  { stream: "stdout", line: "[3/19] Linking C static library esp-idf/esp_hal_security/libesp_hal_security.a" },
  { stream: "stdout", line: "[4/19] Linking C static library esp-idf/esp_hal_mspi/libesp_hal_mspi.a" },
  { stream: "stdout", line: "[5/19] Linking C static library esp-idf/esp_hal_clock/libesp_hal_clock.a" },
  { stream: "stdout", line: "[6/19] Linking C static library esp-idf/esp_hal_gpspi/libesp_hal_gpspi.a" },
  { stream: "stdout", line: "[7/19] Building C object esp-idf/main/CMakeFiles/__idf_main.dir/hello_world_main.c.obj" },
  { stream: "stdout", line: "FAILED: [code=1] esp-idf/main/CMakeFiles/__idf_main.dir/hello_world_main.c.obj " },
  { stream: "stdout", line: "/home/u/.espressif/tools/riscv32-esp-elf/esp-15.2.0_20251204/riscv32-esp-elf/bin/riscv32-esp-elf-gcc -DESP_PLATFORM -DIDF_VER=\\\"v6.0.1\\\" -DSOC_MMU_PAGE_SIZE=CONFIG_MMU_PAGE_SIZE -DSOC_XTAL_FREQ_MHZ=CONFIG_XTAL_FREQ -D_GLIBCXX_HAVE_POSIX_SEMAPHORE -D_GLIBCXX_USE_POSIX_SEMAPHORE -D_GNU_SOURCE -D_POSIX_READER_WRITER_LOCKS -I/tmp/idf-fail/build/config -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_libc/platform_include -I/home/u/.espressif/v6.0.1/esp-idf/components/freertos/config/include -I/home/u/.espressif/v6.0.1/esp-idf/components/freertos/config/include/freertos -I/home/u/.espressif/v6.0.1/esp-idf/components/freertos/config/riscv/include -I/home/u/.espressif/v6.0.1/esp-idf/components/freertos/FreeRTOS-Kernel/include -I/home/u/.espressif/v6.0.1/esp-idf/components/freertos/FreeRTOS-Kernel/portable/riscv/include -I/home/u/.espressif/v6.0.1/esp-idf/components/freertos/FreeRTOS-Kernel/portable/riscv/include/freertos -I/home/u/.espressif/v6.0.1/esp-idf/components/freertos/esp_additions/include -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_hw_support/include -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_hw_support/include/soc -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_hw_support/ldo/include -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_hw_support/debug_probe/include -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_hw_support/etm/include -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_hw_support/mspi_timing_tuning/include -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_hw_support/mspi_timing_tuning/tuning_scheme_impl/include -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_hw_support/power_supply/include -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_hw_support/modem/include -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_hw_support/include/soc/esp32c3 -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_hw_support/port/esp32c3/. -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_hw_support/port/esp32c3/include -I/home/u/.espressif/v6.0.1/esp-idf/components/heap/include -I/home/u/.espressif/v6.0.1/esp-idf/components/heap/tlsf -I/home/u/.espressif/v6.0.1/esp-idf/components/log/include -I/home/u/.espressif/v6.0.1/esp-idf/components/soc/include -I/home/u/.espressif/v6.0.1/esp-idf/components/soc/esp32c3 -I/home/u/.espressif/v6.0.1/esp-idf/components/soc/esp32c3/include -I/home/u/.espressif/v6.0.1/esp-idf/components/soc/esp32c3/register -I/home/u/.espressif/v6.0.1/esp-idf/components/hal/platform_port/include -I/home/u/.espressif/v6.0.1/esp-idf/components/hal/esp32c3/include -I/home/u/.espressif/v6.0.1/esp-idf/components/hal/include -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_rom/include -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_rom/esp32c3/include -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_rom/esp32c3/include/esp32c3 -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_rom/esp32c3 -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_common/include -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_system/include -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_system/port/soc -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_system/port/include/riscv -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_system/port/include/private -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_stdio/include -I/home/u/.espressif/v6.0.1/esp-idf/components/riscv/include -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_hal_gpio/include -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_hal_gpio/esp32c3/include -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_hal_usb/include -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_hal_usb/esp32c3/include -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_hal_pmu/include -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_hal_pmu/esp32c3/include -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_hal_ana_conv/include -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_hal_ana_conv/esp32c3/include -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_hal_dma/include -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_hal_dma/esp32c3/include -I/home/u/.espressif/v6.0.1/esp-idf/components/spi_flash/include -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_hal_mspi/include -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_hal_mspi/esp32c3/include -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_hal_gpspi/include -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_hal_gpspi/esp32c3/include -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_hal_clock/include -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_hal_clock/esp32c3/include -I/home/u/.espressif/v6.0.1/esp-idf/components/esp_blockdev/include @\"/tmp/idf-fail/build/toolchain/cflags\" -fdiagnostics-color=always -ffunction-sections -fdata-sections -Wall -Werror -Wno-error=unused-function -Wno-error=unused-variable -Wno-error=unused-but-set-variable -Wno-error=deprecated-declarations -Wextra -Wno-error=extra -Wno-unused-parameter -Wno-sign-compare -Wno-enum-conversion -gdwarf-4 -ggdb -Og -fno-shrink-wrap -fmacro-prefix-map=/tmp/idf-fail=. -fmacro-prefix-map=/home/u/.espressif/v6.0.1/esp-idf=/IDF -fstrict-volatile-bitfields -fno-jump-tables -fno-tree-switch-conversion -std=gnu23 -Wno-old-style-declaration -fzero-init-padding-bits=all -fno-malloc-dce -MD -MT esp-idf/main/CMakeFiles/__idf_main.dir/hello_world_main.c.obj -MF esp-idf/main/CMakeFiles/__idf_main.dir/hello_world_main.c.obj.d -o esp-idf/main/CMakeFiles/__idf_main.dir/hello_world_main.c.obj -c /tmp/idf-fail/main/hello_world_main.c" },
  { stream: "stdout", line: "/tmp/idf-fail/main/hello_world_main.c: In function 'app_main':" },
  { stream: "stdout", line: "/tmp/idf-fail/main/hello_world_main.c:18:5: error: implicit declaration of function 'undefined_function_here' [-Wimplicit-function-declaration]" },
  { stream: "stdout", line: "   18 |     undefined_function_here();" },
  { stream: "stdout", line: "      |     ^~~~~~~~~~~~~~~~~~~~~~~" },
  { stream: "stdout", line: "[8/19] Linking C static library esp-idf/esp_hal_dma/libesp_hal_dma.a" },
  { stream: "stdout", line: "[9/19] Performing build step for 'bootloader'" },
  { stream: "stdout", line: "[1/141] Building C object esp-idf/esp_rom/CMakeFiles/__idf_esp_rom.dir/patches/esp_rom_crc.c.obj" },
  { stream: "stdout", line: "[2/141] Building C object esp-idf/esp_rom/CMakeFiles/__idf_esp_rom.dir/patches/esp_rom_spiflash.c.obj" },
  { stream: "stdout", line: "... (abridged) ..." },
  { stream: "stdout", line: "[141/141] cd /tmp/idf-fail/build/bootloader && /home/u/.espressif/tools/python/v6.0.1/venv/bin/python /home/u/.espressif/v6.0.1/esp-idf/components/partition_table/check_sizes.py --offset 0x8000 bootloader 0x0 /tmp/idf-fail/build/bootloader/bootloader.bin" },
  { stream: "stdout", line: "Bootloader binary size 0x5250 bytes. 0x2db0 bytes (36%) free." },
  { stream: "stdout", line: "ninja: build stopped: subcommand failed." },
];

/** A successful build's tail, including the size table. */
export const IDF_BUILD_OK: readonly OutputLine[] = [
  { stream: "stdout", line: "[514/514] Generating binary image from built executable" },
  { stream: "stdout", line: "esptool v5.1.0" },
  { stream: "stdout", line: "Creating esp32c3 image..." },
  { stream: "stdout", line: "Merged 2 ELF sections" },
  { stream: "stdout", line: "Successfully created esp32c3 image." },
  { stream: "stdout", line: "Generated /tmp/idf-ok/build/hello_world.bin" },
  { stream: "stdout", line: "[514/514] Completed 'hello_world.elf'" },
  { stream: "stdout", line: "Total sizes:" },
  { stream: "stdout", line: "Used static IRAM:    45000 bytes ( 282072 remain, 13.8% used)" },
  { stream: "stdout", line: "Used stat D/IRAM:    12000 bytes ( 308000 remain,  3.7% used)" },
  { stream: "stdout", line: "Total image size:   121296 bytes (.bin may be padded larger)" },
  { stream: "stdout", line: "Project build complete." },
];
