// bancada_webscope — an oscilloscope the *board* serves over HTTP.
//
// The board samples ADC1 through the ESP-IDF `adc_continuous` DMA driver,
// triggers in software, and serves a self-contained page plus a binary
// sample endpoint. Bancada shows it in the Web tab: the device-browser
// proxy loads `http://<board>/` into the iframe and logs every exchange.
//
//   GET  /          the scope page (HTML+CSS+JS, no external assets)
//   GET  /info      capabilities + calibration table, JSON
//   GET  /data      one triggered frame, raw u16 little-endian
//   GET  /cfg?...   change acquisition / trigger / generator, JSON reply
//
// Nothing here talks to a CDN: the device-browser proxy forwards only to
// the board, so an external asset would simply never load.
//
// WiFi credentials are NOT compiled in. They arrive over serial and live
// in NVS; with none stored (or a failed join) the board raises its own
// access point so it is never unreachable. Serial control lines:
//
//   {"c":"id"}                          reprint the banner
//   {"c":"wifi","ssid":"..","pass":".."} join a network, remember it
//   {"c":"forget"}                      drop stored credentials, go to AP
//   {"c":"ap"}                          force AP mode for this boot
//
// Targets: esp32, esp32s3, esp32c3 (arduino-esp32 3.x / ESP-IDF 5.x).

#include <Arduino.h>
#include <WiFi.h>
#include <WebServer.h>
#include <ESPmDNS.h>
#include <Preferences.h>
#include <string.h>
#include <stdlib.h>
#include "soc/soc_caps.h"
#include "esp_adc/adc_continuous.h"
#include "esp_adc/adc_cali.h"
#include "esp_adc/adc_cali_scheme.h"

#define WEBSCOPE_FW_VERSION "1.0.0"

// ---------------------------------------------------------------- targets --

#if CONFIG_IDF_TARGET_ESP32
  #define SCOPE_CHIP "esp32"
  static const uint8_t kAdcPins[] = { 32, 33, 34, 35, 36, 39 };
  #define GEN_PIN_DEFAULT 25
  #define IN_PIN_DEFAULT 32
#elif CONFIG_IDF_TARGET_ESP32S3
  #define SCOPE_CHIP "esp32s3"
  static const uint8_t kAdcPins[] = { 1, 2, 3, 4, 5, 6, 7, 8, 9, 10 };
  #define GEN_PIN_DEFAULT 5          // waves1's choice
  #define IN_PIN_DEFAULT 4           // its neighbour on the DevKitC-1 header
#elif CONFIG_IDF_TARGET_ESP32C3
  #define SCOPE_CHIP "esp32c3"
  static const uint8_t kAdcPins[] = { 0, 1, 2, 3, 4 };
  #define GEN_PIN_DEFAULT 5
  #define IN_PIN_DEFAULT 4
#else
  #error "bancada_webscope: unsupported target (need esp32 / esp32s3 / esp32c3)"
#endif

static const int      kNumAdcPins = sizeof(kAdcPins) / sizeof(kAdcPins[0]);
static const uint32_t kMaxSps = SOC_ADC_SAMPLE_FREQ_THRES_HIGH;
static const uint32_t kMinSps = SOC_ADC_SAMPLE_FREQ_THRES_LOW;

// A frame is what the page draws; the surplus is the trigger search window.
#define N_SCREEN  512
#define N_SEARCH  (N_SCREEN * 2)
#define N_CAPTURE (N_SCREEN + N_SEARCH)

// ------------------------------------------------------------------ serial --
//
// With CDCOnBoot=cdc, `Serial` is the native USB port and `Serial0` is UART0
// — two different USB-C sockets on a DevKit, and which one is wired up is the
// board's business, not ours. The banner goes to both and commands are read
// from both, so provisioning works on whichever cable is plugged in.

#if defined(ARDUINO_USB_CDC_ON_BOOT) && ARDUINO_USB_CDC_ON_BOOT
  #define SCOPE_DUAL_SERIAL 1
#endif

static void outWrite(const char *s, size_t n) {
  Serial.write((const uint8_t *)s, n);
#if defined(SCOPE_DUAL_SERIAL)
  Serial0.write((const uint8_t *)s, n);
#endif
}

static void outLine(const char *s) {
  outWrite(s, strlen(s));
  outWrite("\n", 1);
}

// ------------------------------------------------------------------- state --

static uint32_t g_sps    = 40000;   // requested; clamped to the chip window
static uint32_t g_effSps = 40000;   // what the driver actually granted
static uint8_t  g_pin    = 0;       // set from kAdcPins in setup()
static uint8_t  g_chan   = 0;
static uint8_t  g_atten  = 3;       // 3 = 12 dB, the widest input range
static uint8_t  g_trigMode = 1;     // 0 free-run, 1 rising, 2 falling
static uint16_t g_trigLevel = 2048; // raw counts

static uint8_t  g_genPin  = GEN_PIN_DEFAULT;
static uint32_t g_genHz   = 1000;
static uint16_t g_genDuty = 512;    // of 1023
static bool     g_genOn   = true;

#define GEN_RES_BITS 10
#define GEN_HZ_MIN 1
#define GEN_HZ_MAX 20000

static uint8_t  s_rawBuf[N_CAPTURE * SOC_ADC_DIGI_RESULT_BYTES];
static uint16_t s_samples[N_CAPTURE];
static uint16_t s_frame[N_SCREEN];

// ------------------------------------------------------------- calibration --

static adc_cali_handle_t s_cali = NULL;

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

// Raw counts to millivolts. Falls back to a nominal full-scale line when the
// chip carries no eFuse calibration — wrong by a few percent, which beats
// refusing to show a trace at all.
static int calMv(int raw) {
  if (s_cali != NULL) {
    int mv = 0;
    if (adc_cali_raw_to_voltage(s_cali, raw, &mv) == ESP_OK) return mv;
  }
#if CONFIG_IDF_TARGET_ESP32
  static const int kFsMv[4] = { 1100, 1500, 2200, 3900 };
#else
  static const int kFsMv[4] = { 950, 1250, 1750, 3100 };
#endif
  return (int)(((int64_t)raw * kFsMv[g_atten & 3]) / 4095);
}

// ------------------------------------------------------------- ADC driver --

static adc_continuous_handle_t s_adc = NULL;

static void adcStop(void) {
  if (s_adc == NULL) return;
  adc_continuous_stop(s_adc);
  adc_continuous_deinit(s_adc);
  s_adc = NULL;
}

static bool pinToChannel(long gpio, uint8_t *outCh) {
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

static bool adcStart(void) {
  adcStop();

  uint32_t sps = g_sps;
  if (sps > kMaxSps) sps = kMaxSps;
  if (sps < kMinSps) sps = kMinSps;
  g_effSps = sps;

  // One conversion frame ≈ 1/8 of a capture, so a capture is a handful of
  // frames and the pool never has to hold more than two captures' worth.
  uint32_t resPerFrame = N_CAPTURE / 8;
  uint32_t frameBytes = resPerFrame * SOC_ADC_DIGI_RESULT_BYTES;
  frameBytes -= frameBytes % SOC_ADC_DIGI_DATA_BYTES_PER_CONV;
  if (frameBytes == 0) frameBytes = SOC_ADC_DIGI_DATA_BYTES_PER_CONV * 16;

  adc_continuous_handle_cfg_t hcfg = {};
  hcfg.max_store_buf_size = frameBytes * 8;
  hcfg.conv_frame_size = frameBytes;
  if (adc_continuous_new_handle(&hcfg, &s_adc) != ESP_OK) { s_adc = NULL; return false; }

  adc_digi_pattern_config_t pattern = {};
  pattern.atten = g_atten;
  pattern.channel = g_chan;
  pattern.unit = ADC_UNIT_1;
  pattern.bit_width = ADC_BITWIDTH_12;

  adc_continuous_config_t ccfg = {};
  ccfg.pattern_num = 1;
  ccfg.adc_pattern = &pattern;
  ccfg.sample_freq_hz = sps;
  ccfg.conv_mode = ADC_CONV_SINGLE_UNIT_1;
#if SOC_ADC_DIGI_RESULT_BYTES == 2
  ccfg.format = ADC_DIGI_OUTPUT_FORMAT_TYPE1;
#else
  ccfg.format = ADC_DIGI_OUTPUT_FORMAT_TYPE2;
#endif
  if (adc_continuous_config(s_adc, &ccfg) != ESP_OK) {
    adc_continuous_deinit(s_adc);
    s_adc = NULL;
    return false;
  }
  if (adc_continuous_start(s_adc) != ESP_OK) {
    adc_continuous_deinit(s_adc);
    s_adc = NULL;
    return false;
  }
  caliCreate(g_atten, g_chan);
  return true;
}

// Fill s_samples with `want` fresh conversions. The pool is flushed first:
// between two page requests the DMA has kept running, and drawing whatever
// was left in the ring shows the user a frame from seconds ago.
static bool adcCapture(uint32_t want) {
  if (s_adc == NULL) return false;
  adc_continuous_flush_pool(s_adc);

  const uint32_t wantBytes = want * SOC_ADC_DIGI_RESULT_BYTES;
  uint32_t have = 0;
  // Deadline covers the slowest legal rate (611 S/s → 1536 samples ≈ 2.5 s).
  const uint32_t deadline = millis() + 6000;
  while (have < wantBytes) {
    if ((int32_t)(millis() - deadline) >= 0) return false;
    uint32_t got = 0;
    esp_err_t r = adc_continuous_read(s_adc, s_rawBuf + have, wantBytes - have, &got, 200);
    if (r == ESP_OK) {
      have += got;
    } else if (r != ESP_ERR_TIMEOUT) {
      return false;
    }
  }

  uint32_t n = 0;
  for (uint32_t off = 0; off + SOC_ADC_DIGI_RESULT_BYTES <= have && n < want;
       off += SOC_ADC_DIGI_RESULT_BYTES) {
    adc_digi_output_data_t *d = (adc_digi_output_data_t *)&s_rawBuf[off];
#if CONFIG_IDF_TARGET_ESP32
    s_samples[n++] = (uint16_t)(d->type1.data & 0x0FFF);
#else
    if (d->type2.unit != 0) continue;
    if ((uint8_t)d->type2.channel != g_chan) continue;
    s_samples[n++] = (uint16_t)(d->type2.data & 0x0FFF);
#endif
  }
  // A short unpack means the buffer held foreign results; redraw next poll
  // rather than paint a frame padded with zeros.
  return n >= want;
}

// Find the trigger edge and copy N_SCREEN samples starting there. Hysteresis
// keeps noise around the level from firing on every sample.
static void buildFrame(uint32_t captured) {
  uint32_t start = 0;
  const int hyst = 30;

  if (g_trigMode != 0 && captured > N_SCREEN) {
    const uint32_t limit = captured - N_SCREEN;
    bool armed = false;
    for (uint32_t i = 1; i < limit; i++) {
      const int v = (int)s_samples[i];
      if (g_trigMode == 1) {
        if (v < (int)g_trigLevel - hyst) armed = true;
        if (armed && v >= (int)g_trigLevel) { start = i; break; }
      } else {
        if (v > (int)g_trigLevel + hyst) armed = true;
        if (armed && v <= (int)g_trigLevel) { start = i; break; }
      }
    }
  }
  memcpy(s_frame, &s_samples[start], N_SCREEN * sizeof(uint16_t));
}

// ------------------------------------------------------------- generator --

static void genApply(void) {
  ledcDetach(g_genPin);
  if (!g_genOn) {
    pinMode(g_genPin, INPUT);
    return;
  }
  if (ledcAttach(g_genPin, g_genHz, GEN_RES_BITS)) {
    ledcWrite(g_genPin, g_genDuty);
  }
}

// ------------------------------------------------------------------ WiFi --

static Preferences s_prefs;
static String s_ip = "";
static const char *s_mode = "none";
static String s_ssid = "";

#define AP_PASSWORD "bancada123"     // WPA2 needs 8+ chars; the SSID is unique
#define MDNS_NAME "bancada-scope"

// The eFuse MAC, not `WiFi.macAddress()`: the latter reads the STA
// interface, which does not exist yet in AP mode and hands back all zeros —
// every board would raise an access point called `bancada-scope-0000`.
static String apSsid(void) {
  const uint64_t mac = ESP.getEfuseMac();
  char buf[32];
  snprintf(buf, sizeof(buf), "bancada-scope-%02X%02X",
           (unsigned)((mac >> 32) & 0xFF), (unsigned)((mac >> 40) & 0xFF));
  return String(buf);
}

static void startAp(void) {
  WiFi.disconnect(true);
  WiFi.mode(WIFI_AP);
  const String ssid = apSsid();
  WiFi.softAP(ssid.c_str(), AP_PASSWORD);
  s_mode = "ap";
  s_ssid = ssid;
  s_ip = WiFi.softAPIP().toString();
}

// Returns true when the join succeeded. `timeoutMs` is generous: a cold DHCP
// lease on a busy access point routinely takes several seconds.
static bool joinSta(const String &ssid, const String &pass, uint32_t timeoutMs) {
  if (ssid.length() == 0) return false;
  WiFi.softAPdisconnect(true);
  WiFi.mode(WIFI_STA);
  WiFi.begin(ssid.c_str(), pass.c_str());
  const uint32_t deadline = millis() + timeoutMs;
  while (WiFi.status() != WL_CONNECTED) {
    if ((int32_t)(millis() - deadline) >= 0) return false;
    delay(100);
  }
  s_mode = "sta";
  s_ssid = ssid;
  s_ip = WiFi.localIP().toString();
  return true;
}

static void printBanner(void) {
  char b[384];
  int n = snprintf(b, sizeof(b),
                   "!BANCADA-WEBSCOPE {\"fw\":\"%s\",\"chip\":\"%s\",\"mode\":\"%s\","
                   "\"ssid\":\"%s\",\"ip\":\"%s\",\"url\":\"http://%s\",\"mdns\":\"http://%s.local\"}",
                   WEBSCOPE_FW_VERSION, SCOPE_CHIP, s_mode, s_ssid.c_str(),
                   s_ip.c_str(), s_ip.c_str(), MDNS_NAME);
  if (n < 0) return;
  outLine(b);
  if (strcmp(s_mode, "ap") == 0) {
    char h[160];
    snprintf(h, sizeof(h), "join wifi \"%s\" (password %s) then open http://%s",
             s_ssid.c_str(), AP_PASSWORD, s_ip.c_str());
    outLine(h);
  }
}

// mDNS has to be torn down and raised again when the interface changes
// (AP → STA), but `end()` on an instance that was never started crashes, and
// starting it in the same breath as a fresh DHCP lease is what produced the
// one boot panic seen on the bench. Track it, and give the stack a moment.
static bool s_mdnsUp = false;

static void restartMdns(void) {
  if (s_mdnsUp) {
    MDNS.end();
    s_mdnsUp = false;
  }
  delay(200);
  if (MDNS.begin(MDNS_NAME)) {
    MDNS.addService("http", "tcp", 80);
    s_mdnsUp = true;
  }
}

static void bringUpNetwork(bool forceAp) {
  s_prefs.begin("webscope", true);
  const String ssid = s_prefs.getString("ssid", "");
  const String pass = s_prefs.getString("pass", "");
  s_prefs.end();

  bool up = false;
  if (!forceAp && ssid.length() > 0) {
    outLine("joining stored wifi...");
    up = joinSta(ssid, pass, 15000);
    if (!up) outLine("join failed — falling back to access point");
  }
  if (!up) startAp();
  restartMdns();
}

// ------------------------------------------------------------------ page --

static const char PAGE[] PROGMEM = R"HTML(<!DOCTYPE html><html lang="en"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Bancada Scope</title><style>
*{box-sizing:border-box}
body{margin:0;background:#0d1117;color:#c9d1d9;font:14px/1.4 system-ui,sans-serif;padding:14px}
h1{font-size:16px;margin:0 0 10px;color:#58a6ff;font-weight:600;letter-spacing:.2px}
h1 small{color:#6e7681;font-weight:400;font-size:12px;margin-left:8px}
.wrap{max-width:1000px;margin:0 auto}
canvas{width:100%;background:#05080d;border:1px solid #21262d;border-radius:8px;display:block}
.med{display:grid;grid-template-columns:repeat(auto-fit,minmax(104px,1fr));gap:8px;margin:12px 0}
.med div{background:#161b22;border:1px solid #21262d;border-radius:6px;padding:7px 10px}
.med span{display:block;color:#8b949e;font-size:11px;text-transform:uppercase;letter-spacing:.5px}
.med b{font-size:17px;color:#3fb950;font-family:ui-monospace,monospace;font-weight:600}
.ctl{display:grid;grid-template-columns:repeat(auto-fit,minmax(196px,1fr));gap:12px}
fieldset{border:1px solid #21262d;border-radius:8px;padding:10px 12px;margin:0;background:#161b22}
legend{color:#58a6ff;font-size:12px;padding:0 5px}
label{display:block;margin:7px 0 3px;font-size:12px;color:#8b949e}
select,input{width:100%;background:#0d1117;color:#c9d1d9;border:1px solid #30363d;border-radius:5px;padding:5px 7px;font:13px inherit}
input[type=range]{padding:0}
input[type=checkbox]{width:auto;vertical-align:middle}
button{background:#238636;color:#fff;border:0;border-radius:6px;padding:9px;width:100%;font:600 14px inherit;cursor:pointer;margin-top:9px}
button.off{background:#da3633}
.st{font-size:12px;color:#8b949e;margin-top:10px;font-family:ui-monospace,monospace}
.st.bad{color:#f85149}
.hint{font-size:11px;color:#6e7681;margin-top:6px;line-height:1.5}
</style></head><body><div class="wrap">
<h1>Bancada Scope <small id="sub">connecting…</small></h1>
<canvas id="cv" width="1000" height="420"></canvas>

<div class="med">
  <div><span>Vpp</span><b id="mVpp">--</b></div>
  <div><span>Max</span><b id="mMax">--</b></div>
  <div><span>Min</span><b id="mMin">--</b></div>
  <div><span>Mean</span><b id="mAvg">--</b></div>
  <div><span>RMS</span><b id="mRms">--</b></div>
  <div><span>Freq</span><b id="mFrq">--</b></div>
  <div><span>Time/div</span><b id="mDiv">--</b></div>
</div>

<div class="ctl">
<fieldset><legend>Acquisition</legend>
  <label>Input pin</label>
  <select id="pin"></select>
  <label>Sample rate</label>
  <select id="sps"></select>
  <label>Input range (attenuation)</label>
  <select id="atten">
    <option value="0">0 dB — up to ~0.95 V</option>
    <option value="1">2.5 dB — up to ~1.25 V</option>
    <option value="2">6 dB — up to ~1.75 V</option>
    <option value="3" selected>12 dB — up to ~3.1 V</option>
  </select>
  <label>Probe factor (divider ahead of the pin)</label>
  <input id="probe" type="number" step="0.1" min="0.1" value="1.0">
  <button id="btRun">STOP</button>
</fieldset>

<fieldset><legend>Trigger</legend>
  <label>Mode</label>
  <select id="trig">
    <option value="1" selected>Rising edge</option>
    <option value="2">Falling edge</option>
    <option value="0">Free run</option>
  </select>
  <label>Level: <b id="lblLvl">1.65 V</b></label>
  <input id="lvl" type="range" min="0" max="4095" value="2048">
  <div class="hint">Level is in volts at the pin, before the probe factor.</div>
</fieldset>

<fieldset><legend>Test generator</legend>
  <label><input type="checkbox" id="genOn" checked> Enabled on <b id="genPin">--</b></label>
  <label>Frequency: <b id="lblF">1000 Hz</b></label>
  <input id="gf" type="range" min="1" max="20000" value="1000">
  <label>Duty: <b id="lblD">50%</b></label>
  <input id="gd" type="range" min="0" max="1023" value="512">
  <div class="hint" id="genHint">&nbsp;</div>
</fieldset>

<fieldset><legend>Display</legend>
  <label>Vertical scale</label>
  <select id="esc">
    <option value="auto" selected>Automatic</option>
    <option value="full">Full input range</option>
  </select>
  <label><input type="checkbox" id="pers"> Persistence</label>
  <label><input type="checkbox" id="grid" checked> Grid</label>
</fieldset>
</div>
<div class="st" id="st">connecting…</div>
</div><script>
const cv=document.getElementById('cv'),ct=cv.getContext('2d');
const W=cv.width,H=cv.height,DX=10,DY=8;
const $=i=>document.getElementById(i);
let running=true,fs=40000,probe=1.0,cal=null,info=null,busy=false;

// The device sends raw 12-bit counts and a 17-point [raw,mV] table; the page
// interpolates. Converting on the board would cost 512 calibration calls per
// frame for a curve the page can walk in a couple of lines.
function mv(raw){
  if(!cal) return raw*3300/4095;
  const step=4096/(cal.length-1);
  let i=Math.floor(raw/step);
  if(i<0)i=0; if(i>cal.length-2)i=cal.length-2;
  const[a,av]=cal[i],[b,bv]=cal[i+1];
  return b===a?av:av+(bv-av)*(raw-a)/(b-a);
}

function grid(){
  ct.fillStyle='#05080d';ct.fillRect(0,0,W,H);
  if(!$('grid').checked) return;
  ct.strokeStyle='#1b2430';ct.lineWidth=1;ct.beginPath();
  for(let i=1;i<DX;i++){ct.moveTo(i*W/DX,0);ct.lineTo(i*W/DX,H);}
  for(let i=1;i<DY;i++){ct.moveTo(0,i*H/DY);ct.lineTo(W,i*H/DY);}
  ct.stroke();
  ct.strokeStyle='#2d3b4d';ct.beginPath();
  ct.moveTo(W/2,0);ct.lineTo(W/2,H);ct.moveTo(0,H/2);ct.lineTo(W,H/2);ct.stroke();
}

const fmtV=v=>Math.abs(v)>=1?v.toFixed(3)+' V':(v*1000).toFixed(1)+' mV';
const fmtT=s=>s>=1e-3?(s*1e3).toFixed(2)+' ms':(s*1e6).toFixed(1)+' µs';

function draw(raw){
  const n=raw.length,v=new Float32Array(n);
  let mx=-9e9,mn=9e9,sum=0,sq=0;
  for(let i=0;i<n;i++){
    const x=mv(raw[i])/1000*probe;
    v[i]=x;sum+=x;sq+=x*x;
    if(x>mx)mx=x; if(x<mn)mn=x;
  }
  const avg=sum/n,rms=Math.sqrt(sq/n);

  // Frequency from mean crossings, hysteresis proportional to amplitude.
  const h=(mx-mn)*0.1;let first=-1,last=-1,count=0,armed=false;
  for(let i=0;i<n;i++){
    if(v[i]<avg-h)armed=true;
    else if(armed&&v[i]>=avg){if(first<0)first=i;last=i;count++;armed=false;}
  }
  const f=count>1?fs*(count-1)/(last-first):0;

  let lo,hi;
  if($('esc').value==='full'){lo=0;hi=mv(4095)/1000*probe;}
  else{const m=(mx-mn)*0.15||0.1;lo=mn-m;hi=mx+m;}

  if($('pers').checked){ct.fillStyle='rgba(5,8,13,.22)';ct.fillRect(0,0,W,H);}
  else grid();

  ct.strokeStyle='#3fb950';ct.lineWidth=2;ct.beginPath();
  for(let i=0;i<n;i++){
    const px=i*W/(n-1),py=H-(v[i]-lo)/(hi-lo)*H;
    i?ct.lineTo(px,py):ct.moveTo(px,py);
  }
  ct.stroke();

  if($('trig').value!=='0'){
    const ly=H-(mv(+$('lvl').value)/1000*probe-lo)/(hi-lo)*H;
    if(ly>=0&&ly<=H){
      ct.strokeStyle='#e8c34a';ct.lineWidth=1;ct.setLineDash([5,4]);
      ct.beginPath();ct.moveTo(0,ly);ct.lineTo(W,ly);ct.stroke();ct.setLineDash([]);
    }
  }

  $('mVpp').textContent=fmtV(mx-mn);
  $('mMax').textContent=fmtV(mx);
  $('mMin').textContent=fmtV(mn);
  $('mAvg').textContent=fmtV(avg);
  $('mRms').textContent=fmtV(rms);
  $('mFrq').textContent=f>0.5?(f>=1000?(f/1000).toFixed(3)+' kHz':f.toFixed(1)+' Hz'):'--';
  $('mDiv').textContent=fmtT(n/fs/DX);
}

async function loop(){
  if(running&&!busy){
    busy=true;
    try{
      const r=await fetch('/data',{cache:'no-store'});
      if(!r.ok)throw new Error('HTTP '+r.status);
      fs=parseInt(r.headers.get('X-Fs'))||fs;
      draw(new Uint16Array(await r.arrayBuffer()));
      $('st').className='st';
      $('st').textContent='running — '+fs+' S/s · pin '+$('pin').value+' · '+
        (cal?'calibrated':'uncalibrated');
    }catch(e){
      $('st').className='st bad';
      $('st').textContent='no data from the board: '+e.message;
    }
    busy=false;
  }
  setTimeout(loop,40);
}

async function cfg(){
  probe=parseFloat($('probe').value)||1;
  const q='/cfg?sps='+$('sps').value+'&pin='+$('pin').value+'&atten='+$('atten').value+
          '&trig='+$('trig').value+'&level='+$('lvl').value+
          '&gen='+($('genOn').checked?1:0)+'&genhz='+$('gf').value+'&genduty='+$('gd').value;
  try{
    const r=await fetch(q,{cache:'no-store'});
    const j=await r.json();
    cal=j.cal;fs=j.sps;
    if(String(j.sps)!==$('sps').value)$('sps').value=String(j.sps);
  }catch(e){/* the poll loop reports the outage */}
}

function fillOptions(j){
  info=j;cal=j.cal;
  $('sub').textContent=j.chip+' · fw '+j.fw+' · '+j.mode+' '+j.ip;
  $('genPin').textContent='GPIO'+j.gen_pin;
  $('pin').innerHTML=j.pins.map(p=>
    '<option value="'+p+'"'+(p===j.pin?' selected':'')+'>GPIO'+p+
    (p===j.gen_pin?' — generator output':'')+'</option>').join('');
  const rates=[500,1000,2000,5000,10000,20000,40000,80000,200000,500000,1000000,2000000]
    .filter(v=>v>=j.min_sps&&v<=j.max_sps);
  if(!rates.includes(j.max_sps))rates.push(j.max_sps);
  $('sps').innerHTML=rates.map(v=>
    '<option value="'+v+'"'+(v===j.sps?' selected':'')+'>'+
    (v>=1000?(v/1000)+' kS/s':v+' S/s')+'</option>').join('');
  if(!rates.includes(j.sps))$('sps').value=String(rates[rates.length-1]);
  $('atten').value=String(j.atten);
  $('trig').value=String(j.trig);
  $('lvl').value=String(j.level);
  $('genOn').checked=!!j.gen;
  $('gf').value=String(j.gen_hz);
  $('gd').value=String(j.gen_duty);
  $('gf').max=String(j.gen_hz_max);
  updateLabels();
}

function updateLabels(){
  $('lblLvl').textContent=(mv(+$('lvl').value)/1000).toFixed(2)+' V';
  $('lblF').textContent=$('gf').value+' Hz';
  $('lblD').textContent=Math.round($('gd').value/1023*100)+'%';
  if(!info)return;
  // A driven pad has no ADC input path, so this pairing reads a flat zero.
  $('genHint').textContent=+$('pin').value===info.gen_pin
    ?'GPIO'+info.gen_pin+' is the generator output — it cannot also be the '
     +'input. Selecting it switches the generator off.'
    :'Jumper GPIO'+info.gen_pin+' to GPIO'+$('pin').value+' to see the test signal.';
}

['sps','pin','atten','trig','lvl','gf','gd','genOn'].forEach(i=>
  $(i).addEventListener('change',cfg));
['lvl','gf','gd'].forEach(i=>$(i).addEventListener('input',updateLabels));
$('pin').addEventListener('change',updateLabels);
$('probe').addEventListener('change',()=>{probe=parseFloat($('probe').value)||1;});
$('btRun').addEventListener('click',e=>{
  running=!running;
  e.target.textContent=running?'STOP':'RUN';
  e.target.className=running?'':'off';
});

grid();
fetch('/info',{cache:'no-store'}).then(r=>r.json()).then(fillOptions)
  .catch(()=>{$('st').className='st bad';$('st').textContent='could not read /info';})
  .finally(loop);
</script></body></html>)HTML";

// ------------------------------------------------------------------ HTTP --

static WebServer server(80);

static void routeRoot(void) {
  server.sendHeader("Cache-Control", "no-store");
  server.send_P(200, "text/html", PAGE);
}

static void routeInfo(void) {
  String js = "{\"fw\":\"" WEBSCOPE_FW_VERSION "\",\"chip\":\"" SCOPE_CHIP "\"";
  js += ",\"mode\":\"" + String(s_mode) + "\",\"ip\":\"" + s_ip + "\"";
  js += ",\"pins\":[";
  for (int i = 0; i < kNumAdcPins; i++) {
    if (i) js += ",";
    js += String((unsigned)kAdcPins[i]);
  }
  js += "],\"pin\":" + String((unsigned)g_pin);
  js += ",\"sps\":" + String((unsigned long)g_effSps);
  js += ",\"min_sps\":" + String((unsigned long)kMinSps);
  js += ",\"max_sps\":" + String((unsigned long)kMaxSps);
  js += ",\"atten\":" + String((unsigned)g_atten);
  js += ",\"trig\":" + String((unsigned)g_trigMode);
  js += ",\"level\":" + String((unsigned)g_trigLevel);
  js += ",\"gen\":" + String(g_genOn ? 1 : 0);
  js += ",\"gen_pin\":" + String((unsigned)g_genPin);
  js += ",\"gen_hz\":" + String((unsigned long)g_genHz);
  js += ",\"gen_duty\":" + String((unsigned)g_genDuty);
  js += ",\"gen_hz_max\":" + String((unsigned long)GEN_HZ_MAX);
  js += ",\"samples\":" + String((unsigned)N_SCREEN);
  js += ",\"cal\":[";
  for (int k = 0; k <= 16; k++) {
    const int raw = k * 256;
    if (k) js += ",";
    js += "[" + String(raw) + "," + String(calMv(raw > 4095 ? 4095 : raw)) + "]";
  }
  js += "]}";
  server.sendHeader("Cache-Control", "no-store");
  server.send(200, "application/json", js);
}

static void routeData(void) {
  const uint32_t want = (g_trigMode == 0) ? (uint32_t)N_SCREEN : (uint32_t)N_CAPTURE;
  if (!adcCapture(want)) {
    server.send(503, "text/plain", "no samples");
    return;
  }
  buildFrame(want);
  server.sendHeader("X-Fs", String((unsigned long)g_effSps));
  server.sendHeader("X-Pin", String((unsigned)g_pin));
  server.sendHeader("Cache-Control", "no-store");
  server.setContentLength(N_SCREEN * 2);
  server.send(200, "application/octet-stream", "");
  server.sendContent((const char *)s_frame, N_SCREEN * 2);
}

static long argOr(const char *name, long fallback) {
  if (!server.hasArg(name)) return fallback;
  return server.arg(name).toInt();
}

static void routeCfg(void) {
  bool restartAdc = false;

  const long sps = argOr("sps", (long)g_sps);
  if (sps > 0 && (uint32_t)sps != g_sps) { g_sps = (uint32_t)sps; restartAdc = true; }

  const long pin = argOr("pin", (long)g_pin);
  if (pin != (long)g_pin) {
    uint8_t ch;
    if (pinToChannel(pin, &ch)) {
      g_pin = (uint8_t)pin;
      g_chan = ch;
      restartAdc = true;
      // Measured on an S3: once LEDC drives a pad, the ADC input path to it
      // is gone and the channel reads ~0. Aiming the input at the generator
      // pin therefore cannot work — switch the generator off rather than
      // hand back a flat line that looks like dead hardware.
      if (g_pin == g_genPin && g_genOn) { g_genOn = false; genApply(); }
    }
  }

  long atten = argOr("atten", (long)g_atten);
  if (atten < 0) atten = 0;
  if (atten > 3) atten = 3;
  if ((uint8_t)atten != g_atten) { g_atten = (uint8_t)atten; restartAdc = true; }

  long trig = argOr("trig", (long)g_trigMode);
  if (trig < 0 || trig > 2) trig = g_trigMode;
  g_trigMode = (uint8_t)trig;

  long level = argOr("level", (long)g_trigLevel);
  if (level < 0) level = 0;
  if (level > 4095) level = 4095;
  g_trigLevel = (uint16_t)level;

  bool genChanged = false;
  const bool genWant = argOr("gen", g_genOn ? 1 : 0) != 0;
  if (genWant != g_genOn) { g_genOn = genWant && (g_genPin != g_pin); genChanged = true; }

  long ghz = argOr("genhz", (long)g_genHz);
  if (ghz < GEN_HZ_MIN) ghz = GEN_HZ_MIN;
  if (ghz > GEN_HZ_MAX) ghz = GEN_HZ_MAX;
  if ((uint32_t)ghz != g_genHz) { g_genHz = (uint32_t)ghz; genChanged = true; }

  long gduty = argOr("genduty", (long)g_genDuty);
  if (gduty < 0) gduty = 0;
  if (gduty > 1023) gduty = 1023;
  if ((uint16_t)gduty != g_genDuty) { g_genDuty = (uint16_t)gduty; genChanged = true; }

  if (genChanged) genApply();
  if (restartAdc && !adcStart()) {
    server.send(500, "application/json", "{\"err\":\"adc driver error\"}");
    return;
  }
  routeInfo();
}

// -------------------------------------------------------- serial control --

static bool jsonStr(const char *s, const char *key, String *out) {
  char pat[24];
  snprintf(pat, sizeof(pat), "\"%s\"", key);
  const char *p = strstr(s, pat);
  if (p == NULL) return false;
  p += strlen(pat);
  while (*p == ' ' || *p == ':') p++;
  if (*p != '"') return false;
  p++;
  String v = "";
  while (*p != '\0' && *p != '"') {
    if (*p == '\\' && p[1] != '\0') p++;  // keep escaped quotes in passwords
    v += *p++;
  }
  if (*p != '"') return false;
  *out = v;
  return true;
}

static void handleCommand(const char *line) {
  String cmd;
  if (!jsonStr(line, "c", &cmd)) { outLine("{\"err\":\"bad command json\"}"); return; }

  if (cmd == "id") {
    printBanner();
  } else if (cmd == "wifi") {
    String ssid, pass;
    if (!jsonStr(line, "ssid", &ssid) || ssid.length() == 0) {
      outLine("{\"err\":\"wifi: missing ssid\"}");
      return;
    }
    jsonStr(line, "pass", &pass);
    outLine("joining...");
    if (joinSta(ssid, pass, 20000)) {
      // Stored only after a join actually worked: remembering a wrong
      // password means every future boot waits out the timeout first.
      s_prefs.begin("webscope", false);
      s_prefs.putString("ssid", ssid);
      s_prefs.putString("pass", pass);
      s_prefs.end();
      restartMdns();
      printBanner();
    } else {
      outLine("{\"err\":\"wifi: join failed\"}");
      startAp();
      printBanner();
    }
  } else if (cmd == "forget") {
    s_prefs.begin("webscope", false);
    s_prefs.clear();
    s_prefs.end();
    startAp();
    printBanner();
  } else if (cmd == "ap") {
    startAp();
    printBanner();
  } else {
    outLine("{\"err\":\"unknown cmd\"}");
  }
}

static char   s_line[192];
static size_t s_lineLen = 0;

static void feedByte(int c) {
  if (c < 0) return;
  if (c == '\n' || c == '\r') {
    if (s_lineLen > 0) {
      s_line[s_lineLen] = '\0';
      s_lineLen = 0;
      handleCommand(s_line);
    }
  } else if (s_lineLen + 1 < sizeof(s_line)) {
    s_line[s_lineLen++] = (char)c;
  } else {
    s_lineLen = 0;  // oversized line: discard rather than truncate into a command
  }
}

static void pollSerial(void) {
  while (Serial.available() > 0) feedByte(Serial.read());
#if defined(SCOPE_DUAL_SERIAL)
  while (Serial0.available() > 0) feedByte(Serial0.read());
#endif
}

// ---------------------------------------------------------------- arduino --

void setup() {
  Serial.begin(115200);
#if defined(SCOPE_DUAL_SERIAL)
  Serial0.begin(115200);
#endif
  delay(400);  // let a native-USB CDC port enumerate before the banner

  // The per-target default sits next to the generator on the header, so the
  // test signal is one short jumper away. Fall back to the first usable pin
  // if a core ever stops offering it.
  g_pin = IN_PIN_DEFAULT;
  uint8_t probe = 0;
  if (!pinToChannel(g_pin, &probe) || g_pin == GEN_PIN_DEFAULT) {
    for (int i = 0; i < kNumAdcPins; i++) {
      if (kAdcPins[i] != GEN_PIN_DEFAULT) { g_pin = kAdcPins[i]; break; }
    }
  }
  uint8_t ch = 0;
  if (pinToChannel(g_pin, &ch)) g_chan = ch;

  genApply();
  if (!adcStart()) outLine("adc driver error at boot");

  bringUpNetwork(false);

  server.on("/", routeRoot);
  server.on("/info", routeInfo);
  server.on("/data", routeData);
  server.on("/cfg", routeCfg);
  server.begin();

  printBanner();
}

void loop() {
  server.handleClient();
  pollSerial();
}
