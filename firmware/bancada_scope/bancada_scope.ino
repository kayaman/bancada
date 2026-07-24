// bancada_scope — ESP32 companion firmware for the Bancada oscilloscope.
//
// Implements the device→host serial protocol from docs/scope-architecture.md §1:
//   - binary frames: sync A5 5A, type, flags, seq, payload_len, first_sample_index,
//     payload, CRC-16/CCITT-FALSE over [type..payload]  (all integers little-endian)
//   - text banner "!BANCADA {...}" on boot and in reply to {"c":"id"}
//   - control plane: newline-delimited JSON commands id/start/stop/single
//
// Uses the ESP-IDF adc_continuous driver directly (NOT the Arduino
// analogContinuousRead() wrapper, which averages each conversion frame).
//
// Targets: esp32, esp32s3, esp32c3 (arduino-esp32 3.x / ESP-IDF 5.x).

#include <Arduino.h>
#include <string.h>
#include <stdlib.h>
#include <stdarg.h>
#include "sdkconfig.h"
#include "soc/soc_caps.h"
#include "esp_adc/adc_continuous.h"
#include "esp_adc/adc_cali.h"
#include "esp_adc/adc_cali_scheme.h"
#include "esp_heap_caps.h"

// ---------------------------------------------------------------- targets --

#define SCOPE_FW_VERSION "0.1.0"
#define SCOPE_PROTO 1

#if CONFIG_IDF_TARGET_ESP32
  #define SCOPE_CHIP "esp32"
  static const uint8_t kAdcPins[] = { 32, 33, 34, 35, 36, 37, 38, 39 };
#elif CONFIG_IDF_TARGET_ESP32S3
  #define SCOPE_CHIP "esp32s3"
  static const uint8_t kAdcPins[] = { 1, 2, 3, 4, 5, 6, 7, 8, 9, 10 };
#elif CONFIG_IDF_TARGET_ESP32C3
  #define SCOPE_CHIP "esp32c3"
  static const uint8_t kAdcPins[] = { 0, 1, 2, 3, 4 };
#else
  #error "bancada_scope: unsupported target (need esp32 / esp32s3 / esp32c3)"
#endif

static const int      kNumAdcPins = sizeof(kAdcPins) / sizeof(kAdcPins[0]);
static const uint32_t kMaxSps   = SOC_ADC_SAMPLE_FREQ_THRES_HIGH;  // burst ceiling
static const uint32_t kMinHwSps = SOC_ADC_SAMPLE_FREQ_THRES_LOW;   // driver low bound

// Transport: native USB CDC vs UART bridge.
#if defined(ARDUINO_USB_CDC_ON_BOOT) && ARDUINO_USB_CDC_ON_BOOT
  #if defined(ARDUINO_USB_MODE) && ARDUINO_USB_MODE
    #define SCOPE_SERIAL_HWCDC 1        // USB-Serial-JTAG peripheral
  #else
    #define SCOPE_SERIAL_USBOTG 1       // TinyUSB CDC (tiny 64 B FIFO)
  #endif
  static const uint32_t kMaxStreamSps = (kMaxSps < 83333u) ? kMaxSps : 83333u;
#else
  #define SCOPE_SERIAL_UART 1
  #define SCOPE_UART_BAUD 921600
  // baud/10 bytes per second budget, 2 bytes per sample.
  static const uint32_t kMaxStreamSps =
      (kMaxSps < (SCOPE_UART_BAUD / 10 / 2)) ? kMaxSps : (SCOPE_UART_BAUD / 10 / 2);
#endif

#if defined(SCOPE_SERIAL_USBOTG)
  static const size_t kTxGateMax = 48;    // TinyUSB FIFO is 64 B; write() drains it
#else
  static const size_t kTxGateMax = 4096;  // whole frames fit the enlarged tx buffer
#endif

// ------------------------------------------------------------------ frames --

#define FRAME_SYNC0 0xA5
#define FRAME_SYNC1 0x5A
#define FT_SAMPLES 0x01
#define FT_META 0x02
#define FT_RECORD_HDR 0x03
#define FT_ERROR 0x7F
#define FLAG_DUAL 0x01
#define FLAG_LAST_IN_RECORD 0x04
#define FLAG_OVERFLOW 0x08

#define FRAME_MAX_PAYLOAD 2048
#define STREAM_SAMPLES_PER_FRAME 512   // 1024 B payload while streaming
#define RECORD_SAMPLES_PER_FRAME 1024  // 2048 B payload for single-shot playback

static uint8_t  s_txBuf[14 + FRAME_MAX_PAYLOAD];  // header(12) + payload + crc(2)
static uint16_t g_seq = 0;
static bool     s_ovfPending = false;   // set overflow bit on next emitted frame
static uint32_t g_ovfTotal = 0;         // pool overflow events + dropped frames

static uint16_t crc16_ccitt_false(const uint8_t *d, size_t n) {
  uint16_t crc = 0xFFFF;
  while (n--) {
    crc ^= (uint16_t)(*d++) << 8;
    for (int i = 0; i < 8; i++) {
      crc = (crc & 0x8000) ? (uint16_t)((crc << 1) ^ 0x1021) : (uint16_t)(crc << 1);
    }
  }
  return crc;
}

// Emit one frame. `payload` may be NULL when the payload was packed directly
// into s_txBuf+12. When `droppable`, the frame is discarded (seq still
// advances, so the host sees the gap) if the serial TX buffer lacks room —
// we never block on Serial while the ADC DMA pool is filling.
static bool emitFrame(uint8_t type, uint8_t flags, uint32_t firstIdx,
                      const void *payload, uint16_t plen, bool droppable) {
  if (plen > FRAME_MAX_PAYLOAD) return false;
  if (payload != NULL && payload != (const void *)(s_txBuf + 12)) {
    memcpy(s_txBuf + 12, payload, plen);
  }
  if (s_ovfPending) flags |= FLAG_OVERFLOW;
  s_txBuf[0] = FRAME_SYNC0;
  s_txBuf[1] = FRAME_SYNC1;
  s_txBuf[2] = type;
  s_txBuf[3] = flags;
  s_txBuf[4] = (uint8_t)(g_seq & 0xFF);
  s_txBuf[5] = (uint8_t)(g_seq >> 8);
  s_txBuf[6] = (uint8_t)(plen & 0xFF);
  s_txBuf[7] = (uint8_t)(plen >> 8);
  s_txBuf[8] = (uint8_t)(firstIdx & 0xFF);
  s_txBuf[9] = (uint8_t)((firstIdx >> 8) & 0xFF);
  s_txBuf[10] = (uint8_t)((firstIdx >> 16) & 0xFF);
  s_txBuf[11] = (uint8_t)((firstIdx >> 24) & 0xFF);
  uint16_t crc = crc16_ccitt_false(s_txBuf + 2, (size_t)10 + plen);
  s_txBuf[12 + plen] = (uint8_t)(crc & 0xFF);
  s_txBuf[13 + plen] = (uint8_t)(crc >> 8);
  size_t total = (size_t)14 + plen;
  g_seq++;
  if (droppable) {
    size_t gate = (total < kTxGateMax) ? total : kTxGateMax;
    if ((size_t)Serial.availableForWrite() < gate) {
      g_ovfTotal++;
      s_ovfPending = true;
      return false;  // dropped whole frame; host sees seq gap + overflow flag
    }
  }
  Serial.write(s_txBuf, total);
  s_ovfPending = false;
  return true;
}

// ------------------------------------------------------------- calibration --

static adc_cali_handle_t s_cali = NULL;
static uint8_t g_atten = 3;

static void caliDestroy(void) {
  if (s_cali == NULL) return;
#if ADC_CALI_SCHEME_CURVE_FITTING_SUPPORTED
  adc_cali_delete_scheme_curve_fitting(s_cali);
#elif ADC_CALI_SCHEME_LINE_FITTING_SUPPORTED
  adc_cali_delete_scheme_line_fitting(s_cali);
#endif
  s_cali = NULL;
}

static void caliCreate(uint8_t atten, uint8_t channel) {
  caliDestroy();
#if ADC_CALI_SCHEME_CURVE_FITTING_SUPPORTED
  adc_cali_curve_fitting_config_t cfg = {};
  cfg.unit_id = ADC_UNIT_1;
  cfg.chan = (adc_channel_t)channel;
  cfg.atten = (adc_atten_t)atten;
  cfg.bitwidth = ADC_BITWIDTH_12;
  if (adc_cali_create_scheme_curve_fitting(&cfg, &s_cali) != ESP_OK) s_cali = NULL;
#elif ADC_CALI_SCHEME_LINE_FITTING_SUPPORTED
  (void)channel;
  adc_cali_line_fitting_config_t cfg = {};
  cfg.unit_id = ADC_UNIT_1;
  cfg.atten = (adc_atten_t)atten;
  cfg.bitwidth = ADC_BITWIDTH_12;
#if CONFIG_IDF_TARGET_ESP32
  cfg.default_vref = 1100;
#endif
  if (adc_cali_create_scheme_line_fitting(&cfg, &s_cali) != ESP_OK) s_cali = NULL;
#else
  (void)atten;
  (void)channel;
#endif
}

static int calMv(int raw) {
  if (s_cali != NULL) {
    int mv = 0;
    if (adc_cali_raw_to_voltage(s_cali, raw, &mv) == ESP_OK) return mv;
  }
  // Approximate linear fallback when the eFuse calibration data is missing:
  // nominal full-scale input per attenuation, which differs per chip family.
#if CONFIG_IDF_TARGET_ESP32
  static const int kFsMv[4] = { 1100, 1500, 2200, 3900 };
#else  // esp32s3 / esp32c3
  static const int kFsMv[4] = { 950, 1250, 1750, 3100 };
#endif
  return (int)(((int64_t)raw * kFsMv[g_atten & 3]) / 4095);
}

// META `cal` point for a table raw value. The table deliberately runs one step
// past full scale (4096 > 4095); clamping the lookup there would leave a flat
// final segment and squash the top of the range, so extend the last real
// segment linearly instead.
static int calMvTablePoint(int raw) {
  if (raw <= 4095) return calMv(raw);
  const int a = calMv(3840);
  const int b = calMv(4095);
  return a + (int)(((int64_t)(b - a) * 256) / 255);
}

// ------------------------------------------------------------- ADC driver --

static adc_continuous_handle_t s_adc = NULL;
static volatile uint32_t s_poolOvfIsr = 0;  // incremented from ISR
static uint32_t s_poolOvfSeen = 0;
static uint8_t  s_readBuf[2048];

static bool IRAM_ATTR onPoolOvf(adc_continuous_handle_t handle,
                                const adc_continuous_evt_data_t *edata,
                                void *user_data) {
  (void)handle; (void)edata; (void)user_data;
  s_poolOvfIsr = s_poolOvfIsr + 1;
  return false;
}

static void adcStop(void) {
  if (s_adc == NULL) return;
  adc_continuous_stop(s_adc);
  adc_continuous_deinit(s_adc);
  s_adc = NULL;
}

// hwSps: hardware aggregate conversion frequency (already clamped/decimated).
static esp_err_t adcSetup(uint32_t hwSps, const uint8_t *channels, int nch, uint8_t atten) {
  adcStop();

  // Conversion frame ≈ 20 ms of data, clamped; multiple of DATA_BYTES_PER_CONV.
  uint32_t resPerFrame = hwSps / 50;
  if (resPerFrame < 64) resPerFrame = 64;
  if (resPerFrame > 1024) resPerFrame = 1024;
  uint32_t frameBytes = resPerFrame * SOC_ADC_DIGI_RESULT_BYTES;
  frameBytes -= frameBytes % SOC_ADC_DIGI_DATA_BYTES_PER_CONV;
  if (frameBytes == 0) frameBytes = SOC_ADC_DIGI_DATA_BYTES_PER_CONV * 16;
  // Keep the store buffer an exact whole number of conversion frames (roughly
  // 4-32 KB) so no ringbuffer space is stranded between frame boundaries.
  uint32_t poolFrames = 8;
  if (frameBytes * poolFrames < 4096) poolFrames = (4096 + frameBytes - 1) / frameBytes;
  if (frameBytes * poolFrames > 32768) poolFrames = 32768 / frameBytes;
  if (poolFrames < 2) poolFrames = 2;
  uint32_t storeBytes = frameBytes * poolFrames;

  adc_continuous_handle_cfg_t hcfg = {};
  hcfg.max_store_buf_size = storeBytes;
  hcfg.conv_frame_size = frameBytes;
  esp_err_t err = adc_continuous_new_handle(&hcfg, &s_adc);
  if (err != ESP_OK) { s_adc = NULL; return err; }

  adc_digi_pattern_config_t pattern[2] = {};
  for (int i = 0; i < nch; i++) {
    pattern[i].atten = atten;
    pattern[i].channel = channels[i];
    pattern[i].unit = ADC_UNIT_1;
    pattern[i].bit_width = ADC_BITWIDTH_12;
  }
  adc_continuous_config_t ccfg = {};
  ccfg.pattern_num = (uint32_t)nch;
  ccfg.adc_pattern = pattern;
  ccfg.sample_freq_hz = hwSps;
  ccfg.conv_mode = ADC_CONV_SINGLE_UNIT_1;
#if SOC_ADC_DIGI_RESULT_BYTES == 2
  ccfg.format = ADC_DIGI_OUTPUT_FORMAT_TYPE1;
#else
  ccfg.format = ADC_DIGI_OUTPUT_FORMAT_TYPE2;
#endif
  err = adc_continuous_config(s_adc, &ccfg);
  if (err != ESP_OK) { adc_continuous_deinit(s_adc); s_adc = NULL; return err; }

  adc_continuous_evt_cbs_t cbs = {};
  cbs.on_pool_ovf = onPoolOvf;
  adc_continuous_register_event_callbacks(s_adc, &cbs, NULL);

  err = adc_continuous_start(s_adc);
  if (err != ESP_OK) { adc_continuous_deinit(s_adc); s_adc = NULL; return err; }
  return ESP_OK;
}

// ---------------------------------------------------------------- session --

enum ScopeState { ST_IDLE, ST_STREAM, ST_ARMED };
static ScopeState s_state = ST_IDLE;

static uint8_t  s_pins[2];      // GPIO numbers of active channels
static uint8_t  s_chans[2];     // ADC1 channel ids of active channels
static int      s_nch = 0;
static uint32_t s_effSps = 0;   // effective per-channel rate (reported in META)
static uint32_t s_decim = 1;    // keep every Nth sample (per channel)
static uint32_t s_decimCnt[2];

// streaming
static uint32_t s_sampleIdx = 0;      // kept samples since capture start
static uint16_t s_frameSamples = 0;   // samples packed in the pending frame
static uint32_t s_frameFirstIdx = 0;
static uint32_t s_lastFlushMs = 0;
static uint32_t s_lastMetaMs = 0;

// single-shot
static uint16_t *s_rec = NULL;
static uint32_t s_pre = 0, s_post = 0;
static uint32_t s_preFill = 0, s_preHead = 0, s_postIdx = 0;
static uint16_t s_level = 2048;
static bool     s_rising = true;
static bool     s_trigArmed = false;
static bool     s_fired = false;
static bool     s_recDone = false;
#define TRIG_HYST 16

static int chanIndex(uint8_t ch) {
  for (int i = 0; i < s_nch; i++) {
    if (s_chans[i] == ch) return i;
  }
  return -1;
}

static bool pinToAdc1Channel(long gpio, uint8_t *outCh) {
  bool listed = false;
  for (int i = 0; i < kNumAdcPins; i++) {
    if (kAdcPins[i] == (uint8_t)gpio) { listed = true; break; }
  }
  if (!listed) return false;
  adc_unit_t unit;
  adc_channel_t ch;
  if (adc_continuous_io_to_channel((int)gpio, &unit, &ch) != ESP_OK) return false;
  if (unit != ADC_UNIT_1) return false;
  *outCh = (uint8_t)ch;
  return true;
}

// -------------------------------------------------------------- meta / err --

static void flushStreamFrame(void) {
  if (s_frameSamples == 0) return;
  uint8_t flags = (s_nch == 2) ? FLAG_DUAL : 0;
  emitFrame(FT_SAMPLES, flags, s_frameFirstIdx, NULL, (uint16_t)(s_frameSamples * 2), true);
  s_frameSamples = 0;
  s_lastFlushMs = millis();
}

static int addJson(char *buf, int n, int cap, const char *fmt, ...) {
  if (n < 0 || n >= cap) return cap;
  va_list ap;
  va_start(ap, fmt);
  int w = vsnprintf(buf + n, (size_t)(cap - n), fmt, ap);
  va_end(ap);
  if (w < 0) return cap;
  n += w;
  return (n > cap) ? cap : n;
}

static void sendMeta(const char *mode, bool done) {
  if (s_state == ST_STREAM) flushStreamFrame();
  char js[768];
  const int cap = (int)sizeof(js);
  int n = 0;
  n = addJson(js, n, cap, "{\"ev\":\"meta\",\"sps\":%lu,\"pins\":[", (unsigned long)s_effSps);
  for (int i = 0; i < s_nch; i++) {
    n = addJson(js, n, cap, "%s%u", i ? "," : "", (unsigned)s_pins[i]);
  }
  n = addJson(js, n, cap, "],\"atten\":%u,\"width\":12,\"mode\":\"%s\",", (unsigned)g_atten, mode);
  if (done) n = addJson(js, n, cap, "\"done\":true,");
  n = addJson(js, n, cap, "\"overflows\":%lu,\"cal\":[", (unsigned long)g_ovfTotal);
  for (int k = 0; k <= 16; k++) {
    int raw = k * 256;  // 17 points: 0, 256, ... 4096
    n = addJson(js, n, cap, "%s[%d,%d]", k ? "," : "", raw, calMvTablePoint(raw));
  }
  n = addJson(js, n, cap, "]}");
  if (n >= cap) n = cap - 1;
  uint8_t flags = (s_nch == 2) ? FLAG_DUAL : 0;
  emitFrame(FT_META, flags, 0, js, (uint16_t)n, false);
  s_lastMetaMs = millis();
}

static void sendErr(const char *msg) {
  if (s_state == ST_STREAM) flushStreamFrame();
  char js[192];
  int n = snprintf(js, sizeof(js), "{\"ev\":\"err\",\"msg\":\"%s\"}", msg);
  if (n < 0) return;
  if (n >= (int)sizeof(js)) n = (int)sizeof(js) - 1;
  emitFrame(FT_ERROR, 0, 0, js, (uint16_t)n, false);
}

static void printBanner(void) {
  char b[320];
  const int cap = (int)sizeof(b);
  int n = 0;
  n = addJson(b, n, cap,
              "!BANCADA {\"proto\":%d,\"chip\":\"%s\",\"fw\":\"%s\","
              "\"max_sps\":%lu,\"max_stream_sps\":%lu,\"pins\":[",
              SCOPE_PROTO, SCOPE_CHIP, SCOPE_FW_VERSION,
              (unsigned long)kMaxSps, (unsigned long)kMaxStreamSps);
  for (int i = 0; i < kNumAdcPins; i++) {
    n = addJson(b, n, cap, "%s%u", i ? "," : "", (unsigned)kAdcPins[i]);
  }
  n = addJson(b, n, cap, "],\"atten\":[0,1,2,3],\"maxch\":2}\n");
  if (n >= cap) n = cap - 1;
  Serial.write((const uint8_t *)b, (size_t)n);
}

// -------------------------------------------------------------- stop paths --

static void stopAll(void) {
  if (s_state == ST_STREAM) flushStreamFrame();
  adcStop();
  if (s_rec != NULL) { free(s_rec); s_rec = NULL; }
  s_state = ST_IDLE;
}

// --------------------------------------------------------------- sampling --

static inline void streamSample(uint16_t raw, int chIdx) {
  if (s_decim > 1) {
    if ((s_decimCnt[chIdx]++ % s_decim) != 0) return;
  }
  if (s_frameSamples == 0) s_frameFirstIdx = s_sampleIdx;
  uint16_t v = (uint16_t)((raw & 0x0FFF) | ((uint16_t)chIdx << 12));
  uint8_t *p = s_txBuf + 12 + 2 * s_frameSamples;
  p[0] = (uint8_t)(v & 0xFF);
  p[1] = (uint8_t)(v >> 8);
  s_sampleIdx++;
  if (++s_frameSamples >= STREAM_SAMPLES_PER_FRAME) flushStreamFrame();
}

static inline void singleSample(uint16_t raw) {
  raw &= 0x0FFF;
  if (s_decim > 1) {
    if ((s_decimCnt[0]++ % s_decim) != 0) return;
  }
  if (s_fired) {
    s_rec[s_pre + s_postIdx] = raw;
    if (++s_postIdx >= s_post) s_recDone = true;
    return;
  }
  if (s_preFill < s_pre) {  // fill the pre-trigger ring before arming
    s_rec[s_preHead] = raw;
    if (++s_preHead == s_pre) s_preHead = 0;
    s_preFill++;
    return;
  }
  bool fire = false;
  if (s_rising) {
    if (s_trigArmed) {
      if (raw >= s_level) fire = true;
    } else if ((int)raw + TRIG_HYST <= (int)s_level) {
      s_trigArmed = true;
    }
  } else {
    if (s_trigArmed) {
      if (raw <= s_level) fire = true;
    } else if ((int)raw >= (int)s_level + TRIG_HYST) {
      s_trigArmed = true;
    }
  }
  if (fire) {
    s_fired = true;
    s_rec[s_pre] = raw;  // triggering sample = first post sample (trigger_idx = pre)
    s_postIdx = 1;
    if (s_postIdx >= s_post) s_recDone = true;
  } else if (s_pre > 0) {
    s_rec[s_preHead] = raw;  // keep rolling the pre ring while waiting
    if (++s_preHead == s_pre) s_preHead = 0;
  }
}

static void finishRecord(void) {
  adcStop();
  char js[160];
  int n = snprintf(js, sizeof(js),
                   "{\"ev\":\"record\",\"trigger_idx\":%lu,\"pre\":%lu,\"post\":%lu,"
                   "\"sps\":%lu,\"pin\":%u}",
                   (unsigned long)s_pre, (unsigned long)s_pre, (unsigned long)s_post,
                   (unsigned long)s_effSps, (unsigned)s_pins[0]);
  if (n < 0) n = 0;
  if (n >= (int)sizeof(js)) n = (int)sizeof(js) - 1;
  emitFrame(FT_RECORD_HDR, 0, 0, js, (uint16_t)n, false);

  uint32_t total = s_pre + s_post;
  uint32_t idx = 0;
  while (idx < total) {
    uint32_t nThis = total - idx;
    if (nThis > RECORD_SAMPLES_PER_FRAME) nThis = RECORD_SAMPLES_PER_FRAME;
    for (uint32_t k = 0; k < nThis; k++) {
      uint32_t i = idx + k;
      uint16_t v;
      if (i < s_pre) {
        uint32_t phys = s_preHead + i;
        if (phys >= s_pre) phys -= s_pre;  // ring head = oldest pre sample
        v = s_rec[phys];
      } else {
        v = s_rec[i];
      }
      v &= 0x0FFF;  // channel nibble 0
      s_txBuf[12 + 2 * k] = (uint8_t)(v & 0xFF);
      s_txBuf[13 + 2 * k] = (uint8_t)(v >> 8);
    }
    idx += nThis;
    uint8_t flags = (idx >= total) ? FLAG_LAST_IN_RECORD : 0;
    emitFrame(FT_SAMPLES, flags, idx - nThis, NULL, (uint16_t)(nThis * 2), false);
  }

  free(s_rec);
  s_rec = NULL;
  s_state = ST_IDLE;
  sendMeta("single", true);  // single-shot complete
}

// Drain everything the DMA has produced. Returns true if any data was read.
static bool drainAdc(void) {
  if (s_adc == NULL) return false;

  uint32_t ovf = s_poolOvfIsr;
  if (ovf != s_poolOvfSeen) {
    g_ovfTotal += ovf - s_poolOvfSeen;
    s_poolOvfSeen = ovf;
    s_ovfPending = true;
  }

  bool any = false;
  uint32_t got = 0;
  // Bounded: the pool is at most 32 KB, so 32 full reads always drain it. The
  // cap guarantees we return to the serial poll (and feed the task watchdog)
  // even if the DMA somehow keeps pace with us.
  int passes = 32;
  while (passes-- > 0 &&
         adc_continuous_read(s_adc, s_readBuf, sizeof(s_readBuf), &got, 0) == ESP_OK) {
    any = true;
    for (uint32_t off = 0; off + SOC_ADC_DIGI_RESULT_BYTES <= got;
         off += SOC_ADC_DIGI_RESULT_BYTES) {
      adc_digi_output_data_t *r = (adc_digi_output_data_t *)&s_readBuf[off];
#if CONFIG_IDF_TARGET_ESP32
      uint16_t raw = r->type1.data;
      uint8_t ch = (uint8_t)r->type1.channel;
#else
      if (r->type2.unit != 0) continue;  // ADC1 only
      uint16_t raw = (uint16_t)r->type2.data;
      uint8_t ch = (uint8_t)r->type2.channel;
#endif
      int idx = chanIndex(ch);
      if (idx < 0) continue;  // corrupt / foreign result — skip
      if (s_state == ST_STREAM) {
        streamSample(raw, idx);
      } else if (s_state == ST_ARMED) {
        if (idx != 0) continue;
        singleSample(raw);
        if (s_recDone) break;
      }
    }
    if (s_recDone) break;
    if (got < sizeof(s_readBuf)) break;  // drained for now; go service serial
  }

  if (s_recDone) {
    s_recDone = false;
    finishRecord();
  }
  return any;
}

// ---------------------------------------------------------- JSON commands --

static bool jsonInt(const char *s, const char *key, long *out) {
  char pat[24];
  snprintf(pat, sizeof(pat), "\"%s\"", key);
  const char *p = strstr(s, pat);
  if (p == NULL) return false;
  p += strlen(pat);
  while (*p == ' ' || *p == ':') p++;
  char *end = NULL;
  long v = strtol(p, &end, 10);
  if (end == p) return false;
  *out = v;
  return true;
}

static bool jsonStr(const char *s, const char *key, char *out, size_t cap) {
  char pat[24];
  snprintf(pat, sizeof(pat), "\"%s\"", key);
  const char *p = strstr(s, pat);
  if (p == NULL) return false;
  p += strlen(pat);
  while (*p == ' ' || *p == ':') p++;
  if (*p != '"') return false;
  p++;
  size_t i = 0;
  while (*p != '\0' && *p != '"' && i + 1 < cap) out[i++] = *p++;
  out[i] = '\0';
  return *p == '"';
}

static int jsonIntArray(const char *s, const char *key, long *out, int maxn) {
  char pat[24];
  snprintf(pat, sizeof(pat), "\"%s\"", key);
  const char *p = strstr(s, pat);
  if (p == NULL) return 0;
  p += strlen(pat);
  while (*p == ' ' || *p == ':') p++;
  if (*p != '[') return 0;
  p++;
  int n = 0;
  while (n < maxn) {
    char *end = NULL;
    long v = strtol(p, &end, 10);
    if (end == p) break;
    out[n++] = v;
    p = end;
    while (*p == ' ' || *p == ',') p++;
    if (*p == ']') break;
  }
  return n;
}

static void resetCaptureCounters(void) {
  s_sampleIdx = 0;
  s_frameSamples = 0;
  s_frameFirstIdx = 0;
  s_decimCnt[0] = 0;
  s_decimCnt[1] = 0;
  s_lastFlushMs = millis();
}

static void cmdStart(const char *line) {
  long sps = 0;
  if (!jsonInt(line, "sps", &sps) || sps <= 0) { sendErr("start: bad sps"); return; }
  long pinsArr[2] = { 0, 0 };
  int nch = jsonIntArray(line, "pins", pinsArr, 2);
  if (nch < 1) { sendErr("start: need 1-2 pins"); return; }
  long atten = 3;
  jsonInt(line, "atten", &atten);
  if (atten < 0) atten = 0;
  if (atten > 3) atten = 3;

  uint8_t chans[2] = { 0, 0 };
  for (int i = 0; i < nch; i++) {
    if (!pinToAdc1Channel(pinsArr[i], &chans[i])) { sendErr("start: pin is not ADC1"); return; }
  }
  if (nch == 2 && chans[0] == chans[1]) { sendErr("start: duplicate pin"); return; }

  stopAll();

  // Clamp per-channel rate to chip + transport limits, then decimate below
  // the driver's low bound so the effective rate matches the request.
  uint32_t eff = (uint32_t)sps;
  uint32_t capChip = kMaxSps / (uint32_t)nch;
  uint32_t capLink = kMaxStreamSps / (uint32_t)nch;
  if (eff > capChip) eff = capChip;
  if (eff > capLink) eff = capLink;
  if (eff == 0) eff = 1;
  uint32_t total = eff * (uint32_t)nch;
  uint32_t decim = 1;
  if (total < kMinHwSps) decim = (kMinHwSps + total - 1) / total;
  uint32_t hwSps = total * decim;

  for (int i = 0; i < nch; i++) { s_pins[i] = (uint8_t)pinsArr[i]; s_chans[i] = chans[i]; }
  s_nch = nch;
  s_effSps = eff;
  s_decim = decim;
  g_atten = (uint8_t)atten;
  resetCaptureCounters();

  esp_err_t err = adcSetup(hwSps, s_chans, s_nch, g_atten);
  if (err != ESP_OK) {
    s_nch = 0;
    s_effSps = 0;
    sendErr("start: adc driver error");
    return;
  }
  caliCreate(g_atten, s_chans[0]);
  s_state = ST_STREAM;
  sendMeta("stream", false);  // echoes the real (clamped) rate
}

static void cmdSingle(const char *line) {
  long sps = 0;
  if (!jsonInt(line, "sps", &sps) || sps <= 0) { sendErr("single: bad sps"); return; }
  long pin = -1;
  if (!jsonInt(line, "pin", &pin)) { sendErr("single: missing pin"); return; }
  long atten = 3, pre = 1024, post = 4096, level = 2048;
  jsonInt(line, "atten", &atten);
  jsonInt(line, "pre", &pre);
  jsonInt(line, "post", &post);
  jsonInt(line, "level", &level);
  char edge[4] = "r";
  jsonStr(line, "edge", edge, sizeof(edge));
  if (atten < 0) atten = 0;
  if (atten > 3) atten = 3;
  if (pre < 0) pre = 0;
  if (post < 1) post = 1;
  if (level < 0) level = 0;
  if (level > 4095) level = 4095;

  uint8_t chan = 0;
  if (!pinToAdc1Channel(pin, &chan)) { sendErr("single: pin is not ADC1"); return; }

  stopAll();

  // Cap the record to free heap; hard limit 64k samples (128 KB).
  size_t largest = heap_caps_get_largest_free_block(MALLOC_CAP_INTERNAL | MALLOC_CAP_8BIT);
  uint32_t capSamples = 0;
  if (largest > 49152) capSamples = (uint32_t)((largest - 49152) / 2);
  if (capSamples > 65536) capSamples = 65536;
  if (capSamples < 16) { sendErr("single: out of memory"); return; }
  uint64_t want = (uint64_t)pre + (uint64_t)post;
  if (want > capSamples) {
    uint32_t newPre = (uint32_t)(((uint64_t)pre * capSamples) / want);
    uint32_t newPost = capSamples - newPre;
    if (newPost < 1) { newPost = 1; if (newPre > 0) newPre--; }
    pre = (long)newPre;
    post = (long)newPost;
  }

  s_rec = (uint16_t *)malloc((size_t)(pre + post) * 2);
  if (s_rec == NULL) { sendErr("single: out of memory"); return; }

  uint32_t eff = (uint32_t)sps;
  if (eff > kMaxSps) eff = kMaxSps;
  uint32_t decim = 1;
  if (eff < kMinHwSps) decim = (kMinHwSps + eff - 1) / eff;
  uint32_t hwSps = eff * decim;

  s_pins[0] = (uint8_t)pin;
  s_chans[0] = chan;
  s_nch = 1;
  s_effSps = eff;
  s_decim = decim;
  g_atten = (uint8_t)atten;
  s_pre = (uint32_t)pre;
  s_post = (uint32_t)post;
  s_preFill = 0;
  s_preHead = 0;
  s_postIdx = 0;
  s_trigArmed = false;
  s_fired = false;
  s_recDone = false;
  s_rising = (edge[0] != 'f');
  // keep the level inside the hysteresis band so the trigger can always arm
  if (level < TRIG_HYST) level = TRIG_HYST;
  if (level > 4095 - TRIG_HYST) level = 4095 - TRIG_HYST;
  s_level = (uint16_t)level;
  resetCaptureCounters();

  esp_err_t err = adcSetup(hwSps, s_chans, 1, g_atten);
  if (err != ESP_OK) {
    free(s_rec);
    s_rec = NULL;
    sendErr("single: adc driver error");
    return;
  }
  caliCreate(g_atten, s_chans[0]);
  s_state = ST_ARMED;
  sendMeta("single", false);  // echoes clamped sps; RECORD_HDR echoes real pre/post
}

static void handleCommand(const char *line) {
  char cmd[16];
  if (!jsonStr(line, "c", cmd, sizeof(cmd))) {
    sendErr("bad command json");
    return;
  }
  if (strcmp(cmd, "id") == 0) {
    stopAll();  // the banner never appears mid-binary-stream
    printBanner();
  } else if (strcmp(cmd, "stop") == 0) {
    stopAll();
  } else if (strcmp(cmd, "start") == 0) {
    cmdStart(line);
  } else if (strcmp(cmd, "single") == 0) {
    cmdSingle(line);
  } else {
    sendErr("unknown cmd");
  }
}

// ------------------------------------------------------------ serial input --

static char   s_line[256];
static size_t s_lineLen = 0;

static void pollSerial(void) {
  while (Serial.available() > 0) {
    int c = Serial.read();
    if (c < 0) break;
    if (c == '\n' || c == '\r') {
      if (s_lineLen > 0) {
        s_line[s_lineLen] = '\0';
        s_lineLen = 0;
        handleCommand(s_line);
      }
    } else if (s_lineLen + 1 < sizeof(s_line)) {
      s_line[s_lineLen++] = (char)c;
    } else {
      s_lineLen = 0;  // oversized line: discard
    }
  }
}

// ----------------------------------------------------------------- arduino --

void setup() {
#if defined(SCOPE_SERIAL_UART)
  Serial.setTxBufferSize(8192);
  Serial.setRxBufferSize(512);
  Serial.begin(SCOPE_UART_BAUD);
#elif defined(SCOPE_SERIAL_HWCDC)
  Serial.setTxBufferSize(8192);
  Serial.begin();
#else  // USB-OTG TinyUSB CDC
  Serial.begin();
  Serial.setTxTimeoutMs(300);
#endif
  delay(300);  // let USB CDC enumerate before the boot banner
  printBanner();
}

void loop() {
  pollSerial();

  bool worked = false;
  if (s_state == ST_STREAM || s_state == ST_ARMED) {
    worked = drainAdc();
    uint32_t now = millis();
    if (s_state == ST_STREAM && s_frameSamples > 0 && (now - s_lastFlushMs) >= 25) {
      flushStreamFrame();
    }
    if (s_state != ST_IDLE && (now - s_lastMetaMs) >= 1000) {
      sendMeta(s_state == ST_STREAM ? "stream" : "single", false);
    }
  }
  if (!worked) delay(1);  // yield (keeps the idle task fed on single-core C3)
}
