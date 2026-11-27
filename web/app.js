// SlopeGuard dashboard. No framework, no build step: one poll, two canvases.
const $ = (sel) => document.querySelector(sel);
const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));

const state = {
  data: null,
  cursor: null, // index into movement points, null = live
  error: null,
};

const LEVEL_DEG = 2.0; // mirrors analysis::BaselineConfig::default().relative_pitch_deg
const TILT_DRAW = 4; // visual exaggeration, labelled on the scene
const WINDOW_S = 1800;

const I18N = {
  en: {
    "page.title": "SlopeGuard — slope movement watch",
    "skip": "Skip to evidence",
    "brand.sub": "rainfall-induced slope-movement early warning",
    "help.shortcuts": "Guide",
    "nav.demo": "Demo",
    "help.open.title": "Open the guide",
    "lang.switch": "Switch to Bahasa Indonesia",
    "theme.light": "Light",
    "theme.dark": "Dark",
    "theme.light.title": "Switch to light theme",
    "theme.dark.title": "Switch to dark theme",
    "site.none": "no site",
    "ingest.up": "connected",
    "ingest.down": "offline",
    "ingest.wait": "connecting …",
    "clock.title": "site clock, latest packet",
    "svg.title": "Site cross-section",
    "svg.desc": "A soil slope over bedrock. The movement node stands in the soil, the reference node on bedrock. Both masts tilt with their measured angle; rain and the water table follow the telemetry.",
    "svg.reference": "REFERENCE",
    "svg.movement": "MOVEMENT",
    "scene.note": "tilt angles exaggerated ×4 to stay visible",
    "scene.level": "true level",
    "scene.water": "water table",
    "scene.travel": "possible travel",
    "legend.tilt": "Tilt",
    "legend.baseline": "2° baseline",
    "legend.cursor": "Time cursor",
    "legend.rain": "Rain per packet",
    "legend.soil": "Soil moisture",
    "legend.rainOnset": "rain onset",
    "legend.tiltCross": "2° crossed",
    "verdict.loading": "Connecting to the site…",
    "reasons.aria": "Reason codes",
    "gauges.rel": "Relative tilt",
    "gauges.rain": "Rain · 15 min",
    "gauges.soil": "Soil moisture",
    "gauges.rate": "Tilt rate",
    "unit.rate": "°/min",
    "trend.title": "Risk trend",
    "trend.sub": "movement − reference, degrees",
    "trend.aria": "Relative tilt over time with the 2 degree baseline",
    "trend.empty": "baseline 2.0° · no data yet",
    "load.title": "Loading",
    "load.sub": "rain per interval · soil %",
    "load.aria": "Rainfall bars and soil moisture over time",
    "load.empty": "no packets in this window",
    "more.summary": "Sensors & system",
    "nodes.caption": "Sensor health",
    "nodes.node": "Sensor",
    "nodes.seq": "Packet",
    "nodes.age": "Seen",
    "nodes.batt": "Battery",
    "nodes.rssi": "Signal",
    "nodes.empty": "no accepted packets yet",
    "role.movement": "slope sensor",
    "role.reference": "reference sensor",
    "facts.rule": "How it decides",
    "facts.horizon": "Forecast window",
    "facts.model": "AI model",
    "facts.ingest": "Connection",
    "facts.rejects": "Bad packets",
    "facts.rejects.none": "no rejects recorded",
    "facts.rule.value": "rule-based (no AI)",
    "facts.horizon.value": "30 min",
    "model.live": "AI model running",
    "model.report": "AI report ready, rule used",
    "model.none": "AI not connected, rule used",
    "quality.MISSING": "no sensor data",
    "quality.DEGRADED": "sensor data unreliable",
    "strip.live": "Live",
    "strip.storm": "Storm demo",
    "strip.storm.title": "Replay a recorded storm to watch the dashboard react",
    "strip.dry": "Calm demo",
    "strip.dry.title": "Replay a calm recording to see a normal day",
    "strip.liveEdge": "LIVE EDGE",
    "strip.viewing": "VIEWING {time}",
    "scrub.aria": "Time cursor, drag to inspect a moment",
    "mark.rain": "rain onset {time}",
    "mark.tilt": "2° crossed {time}",
    "chart.noPackets": "no movement packets in this window",
    "chart.soil": "soil %",
    "chart.bars": "bars rainfall per 10 s packet (peak <b>{peak} mm</b>) · line soil <b>{from} → {to}</b>",
    "chart.baseline": "baseline <b>{thr}°</b> · now <b>{rel}</b> · {n} packets above",
    "chart.baseline1": "baseline <b>{thr}°</b> · now <b>{rel}</b> · 1 packet above",
    "state.STABLE": "STABLE",
    "state.WATCH": "WATCH",
    "state.MOVEMENT_RISK": "MOVEMENT RISK",
    "state.CRITICAL": "CRITICAL",
    "state.UNKNOWN": "UNVERIFIED",
    "sentence.STABLE": "Movement stays inside the baseline. Keep watching.",
    "sentence.WATCH": "Rain or soil moisture is rising. No confirmed movement yet.",
    "sentence.MOVEMENT_RISK": "Relative tilt is above baseline and still rising.",
    "sentence.CRITICAL": "Independent movement evidence recorded.",
    "sentence.UNKNOWN": "Sensor data is missing or stale, so SlopeGuard cannot judge the slope right now.",
    "reason.NO_ABNORMAL_SIGNAL": "no abnormal signal",
    "reason.REL_TILT_ABOVE_BASELINE": "relative tilt ≥ 2.0°",
    "reason.TILT_RATE_RISING": "tilt rate ≥ 0.2°/min",
    "reason.RAINFALL_LOADING": "rain ≥ 5 mm / 15 min",
    "reason.SOIL_MOISTURE_RISING": "soil moisture rising",
    "reason.REFERENCE_STALE": "reference sensor is stale or faulty",
    "reason.NO_DATA": "no sensor packets in the window",
    "reason.DB_UNAVAILABLE": "data store unavailable",
    "verdict.apiDown": "The dashboard service cannot be reached. Retrying every 2 seconds.",
    "verdict.dbMissing": "The data store is not ready. Try a demo below.",
    "verdict.noData": "No sensor data yet. Make sure the sensor is powered and connected — or try a demo below.",
    "stale.none": "NO DATA YET — nothing has arrived. Try a demo below or power the sensor.",
    "stale.fault": "SENSOR FAULT — {node} reports {status}. Measurements are not trusted.",
    "stale.stale": "STALE — newest packet {age} old. Values shown are the last known, not current.",
    "help.title": "Guide",
    "help.intro": "SlopeGuard watches a hillside and tells you, in plain words, whether the slope is stable or starting to move.",
    "help.legend.stable": "no concerning movement",
    "help.legend.watch": "rain or soil is loading the slope",
    "help.legend.risk": "slope is tilting and rising",
    "help.legend.unknown": "data is missing or stale",
    "help.tour": "Show me around",
    "help.keys": "Keyboard shortcuts",
    "help.l": "Return the cursor to live",
    "help.arrows": "Step through the recording",
    "help.q": "Open or close this panel",
    "help.note": "This verdict comes from a simple rule, not a trained AI model. Demo recordings are not field results.",
    "help.close": "Close",
    "lang.btn.toId": "Bahasa",
    "lang.btn.toEn": "English",
    "tour.count": "{n} of {total}",
    "tour.next": "Next",
    "tour.back": "Back",
    "tour.skip": "Skip tour",
    "tour.done": "Finish",
    "tour.verdict.title": "The verdict",
    "tour.verdict.body": "The big word is the current call. STABLE means no concerning movement; WATCH means rain or soil is loading the slope; MOVEMENT RISK means the slope is tilting. UNVERIFIED means data is missing or stale, so no call is possible. The sentence below explains why.",
    "tour.gauges.title": "Four numbers",
    "tour.gauges.body": "Tilt compares the slope sensor with the reference sensor. Rain is the last 15 minutes. Soil is moisture at the probe. Rate is how fast tilt changes. Hover a chart to see exact numbers.",
    "tour.evidence.title": "History",
    "tour.scene.title": "The drawing",
    "tour.scene.body": "The slope sensor (orange) tilts downhill as the soil moves; the reference sensor stays on stable rock. The dashed line is true level, the blue dotted curve is the water table, and the dashed orange curve shows possible travel.",
    "tour.evidence.body": "Top chart: solid orange is tilt, dashed orange is the 2° baseline, dotted pale is the time cursor. Bottom chart: blue bars are rain per packet, the thin line is soil moisture. Drag the timeline below to look back in time.",
    "tour.more.title": "Sensors & system",
    "tour.more.body": "Open this to see when each sensor last reported, its battery and signal, plus how decisions are made. The slope sensor sits in the soil; the reference sensor sits on stable bedrock.",
    "tour.strip.title": "Timeline",
    "tour.strip.body": "Drag the slider to review the last 30 minutes; Live returns to now. Rain onset and the 2° crossing are marked on the track. Demos have their own page, opened from the Demo button in the header.",
    "tour.lang.title": "Language & theme",
    "tour.lang.body": "Use these buttons to switch between English and Bahasa Indonesia and between the light and dark theme. Your choices are remembered.",
    "noscript": "This dashboard needs JavaScript. The API is still readable at",
  },
  id: {
    "page.title": "SlopeGuard — pemantauan pergerakan lereng",
    "skip": "Langsung ke bukti",
    "brand.sub": "peringatan dini pergerakan lereng akibat hujan",
    "help.shortcuts": "Panduan",
    "nav.demo": "Demo",
    "help.open.title": "Buka panduan",
    "lang.switch": "Ganti ke Bahasa Inggris",
    "theme.light": "Terang",
    "theme.dark": "Gelap",
    "theme.light.title": "Ganti ke tema terang",
    "theme.dark.title": "Ganti ke tema gelap",
    "site.none": "tanpa situs",
    "ingest.up": "online",
    "ingest.down": "offline",
    "ingest.wait": "menghubungkan …",
    "clock.title": "jam situs, paket terbaru",
    "svg.title": "Penampang situs",
    "svg.desc": "Lereng tanah di atas batuan dasar. Sensor pergerakan tertanam di tanah, sensor referensi di batuan dasar. Kedua tiang miring sesuai sudut terukur; hujan dan muka air tanah mengikuti data sensor.",
    "svg.reference": "REFERENSI",
    "svg.movement": "PERGERAKAN",
    "scene.note": "sudut tilt diperbesar ×4 agar terlihat jelas",
    "scene.level": "level sebenarnya",
    "scene.water": "muka air tanah",
    "scene.travel": "potensi pergerakan",
    "legend.tilt": "Tilt",
    "legend.baseline": "baseline 2°",
    "legend.cursor": "Kursor waktu",
    "legend.rain": "Hujan per paket",
    "legend.soil": "Kelembapan tanah",
    "legend.rainOnset": "awal hujan",
    "legend.tiltCross": "lewat 2°",
    "verdict.loading": "Menghubungkan ke situs…",
    "reasons.aria": "Kode alasan",
    "gauges.rel": "Tilt relatif",
    "gauges.rain": "Hujan · 15 menit",
    "gauges.soil": "Kelembapan tanah",
    "gauges.rate": "Laju tilt",
    "unit.rate": "°/menit",
    "trend.title": "Tren risiko",
    "trend.sub": "pergerakan − referensi, derajat",
    "trend.aria": "Tilt relatif dari waktu ke waktu dengan baseline 2 derajat",
    "trend.empty": "baseline 2.0° · belum ada data",
    "load.title": "Beban",
    "load.sub": "hujan per interval · kelembapan %",
    "load.aria": "Batang hujan dan kelembapan tanah dari waktu ke waktu",
    "load.empty": "tidak ada paket di jendela ini",
    "more.summary": "Sensor & sistem",
    "nodes.caption": "Kesehatan sensor",
    "nodes.node": "Sensor",
    "nodes.seq": "Paket",
    "nodes.age": "Terakhir",
    "nodes.batt": "Baterai",
    "nodes.rssi": "Sinyal",
    "nodes.empty": "belum ada paket diterima",
    "role.movement": "sensor slope",
    "role.reference": "sensor referensi",
    "facts.rule": "Dasar keputusan",
    "facts.horizon": "Horizon prediksi",
    "facts.model": "Model AI",
    "facts.ingest": "Koneksi",
    "facts.rejects": "Paket ditolak",
    "facts.rejects.none": "belum ada penolakan",
    "facts.rule.value": "berbasis aturan (belum AI)",
    "facts.horizon.value": "30 menit",
    "model.live": "model AI aktif",
    "model.report": "laporan AI siap, aturan dipakai",
    "model.none": "AI belum terhubung, aturan dipakai",
    "quality.MISSING": "belum ada data sensor",
    "quality.DEGRADED": "data sensor tidak valid",
    "strip.live": "Live",
    "strip.storm": "Demo badai",
    "strip.storm.title": "Putar rekaman badai untuk melihat reaksi dashboard",
    "strip.dry": "Demo tenang",
    "strip.dry.title": "Putar rekaman hari tenang untuk melihat kondisi normal",
    "strip.liveEdge": "LIVE",
    "strip.viewing": "LIHAT {time}",
    "scrub.aria": "Kursor waktu, geser untuk melihat momen tertentu",
    "mark.rain": "awal hujan {time}",
    "mark.tilt": "melewati 2° {time}",
    "chart.noPackets": "tidak ada paket pergerakan di jendela ini",
    "chart.soil": "kelembapan %",
    "chart.bars": "batang = hujan per paket 10 detik (puncak <b>{peak} mm</b>) · garis = kelembapan <b>{from} → {to}</b>",
    "chart.baseline": "baseline <b>{thr}°</b> · sekarang <b>{rel}</b> · {n} paket di atas",
    "chart.baseline1": "baseline <b>{thr}°</b> · sekarang <b>{rel}</b> · 1 paket di atas",
    "state.STABLE": "AMAN",
    "state.WATCH": "WASPADA",
    "state.MOVEMENT_RISK": "RISIKO PERGERAKAN",
    "state.CRITICAL": "KRITIS",
    "state.UNKNOWN": "DATA TIDAK VALID",
    "sentence.STABLE": "Pergerakan masih di bawah baseline. Tetap pantau.",
    "sentence.WATCH": "Curah hujan atau kelembapan tanah naik. Belum ada pergerakan yang terkonfirmasi.",
    "sentence.MOVEMENT_RISK": "Tilt relatif di atas baseline dan masih naik.",
    "sentence.CRITICAL": "Terdeteksi bukti pergerakan independen.",
    "sentence.UNKNOWN": "Data sensor hilang atau kedaluwarsa, jadi SlopeGuard belum bisa menilai lereng saat ini.",
    "reason.NO_ABNORMAL_SIGNAL": "tidak ada sinyal abnormal",
    "reason.REL_TILT_ABOVE_BASELINE": "tilt relatif ≥ 2.0°",
    "reason.TILT_RATE_RISING": "laju tilt ≥ 0.2°/menit",
    "reason.RAINFALL_LOADING": "hujan ≥ 5 mm / 15 menit",
    "reason.SOIL_MOISTURE_RISING": "kelembapan tanah naik",
    "reason.REFERENCE_STALE": "sensor referensi tidak mengirim data baru",
    "reason.NO_DATA": "tidak ada paket sensor di jendela",
    "reason.DB_UNAVAILABLE": "database tidak tersedia",
    "verdict.apiDown": "Layanan dashboard tidak dapat dihubungi. Mencoba lagi tiap 2 detik.",
    "verdict.dbMissing": "Database belum siap. Coba demo di bawah.",
    "verdict.noData": "Belum ada data sensor. Pastikan sensor menyala dan terhubung — atau coba demo di bawah.",
    "stale.none": "BELUM ADA DATA — belum ada data yang masuk. Coba demo di bawah atau nyalakan sensor.",
    "stale.fault": "GANGGUAN SENSOR — {node} melaporkan {status}. Pengukuran tidak dapat dipercaya.",
    "stale.stale": "DATA LAMA — paket terbaru berumur {age}. Nilai yang tampil adalah data terakhir, bukan kondisi saat ini.",
    "help.title": "Panduan",
    "help.intro": "SlopeGuard memantau lereng dan memberi tahu dengan bahasa sederhana apakah lereng aman atau mulai bergerak.",
    "help.legend.stable": "tidak ada pergerakan yang mengkhawatirkan",
    "help.legend.watch": "hujan atau tanah membebani lereng",
    "help.legend.risk": "lereng miring dan terus naik",
    "help.legend.unknown": "data hilang atau sudah lama",
    "help.tour": "Tunjukkan cara pakai",
    "help.keys": "Shortcut keyboard",
    "help.l": "Kembalikan kursor ke mode live",
    "help.arrows": "Maju atau mundur satu paket data",
    "help.q": "Buka atau tutup panel ini",
    "help.note": "Status ini berasal dari aturan sederhana, bukan model AI terlatih. Rekaman demo bukan hasil lapangan.",
    "help.close": "Tutup",
    "lang.btn.toId": "Bahasa",
    "lang.btn.toEn": "English",
    "tour.count": "{n} dari {total}",
    "tour.next": "Lanjut",
    "tour.back": "Kembali",
    "tour.skip": "Lewati tur",
    "tour.done": "Selesai",
    "tour.verdict.title": "Status",
    "tour.verdict.body": "Kata besar ini adalah status saat ini. AMAN berarti tidak ada pergerakan yang mengkhawatirkan; WASPADA berarti hujan atau tanah sedang membebani lereng; RISIKO PERGERAKAN berarti lereng mulai miring. DATA TIDAK VALID berarti data hilang atau kedaluwarsa sehingga belum bisa dinilai. Kalimat di bawahnya menjelaskan alasannya.",
    "tour.gauges.title": "Empat angka",
    "tour.gauges.body": "Tilt membandingkan sensor slope dengan sensor referensi. Hujan adalah 15 menit terakhir. Tanah adalah kelembapan di probe. Laju adalah kecepatan perubahan tilt. Arahkan kursor ke grafik untuk angka pastinya.",
    "tour.evidence.title": "Riwayat",
    "tour.scene.title": "Ilustrasi lereng",
    "tour.scene.body": "Sensor slope (oranye) miring ke bawah saat tanah bergerak; sensor referensi tetap di batuan stabil. Garis putus-putus adalah level sebenarnya, kurva biru titik-titik adalah muka air tanah, dan kurva oranye putus-putus menunjukkan potensi pergerakan.",
    "tour.evidence.body": "Grafik atas: oranye solid adalah tilt, oranye putus-putus adalah baseline 2°, titik-titik pucat adalah kursor waktu. Grafik bawah: batang biru adalah hujan per paket, garis tipis adalah kelembapan tanah. Geser timeline di bawah untuk melihat ke belakang.",
    "tour.more.title": "Sensor & sistem",
    "tour.more.body": "Buka bagian ini untuk melihat kapan tiap sensor terakhir mengirim data, baterai dan sinyalnya, serta dasar keputusan. Sensor slope tertanam di tanah; sensor referensi di batuan dasar yang stabil.",
    "tour.strip.title": "Timeline",
    "tour.strip.body": "Geser slider untuk meninjau 30 menit terakhir; Live kembali ke sekarang. Awal hujan dan saat melewati 2° ditandai di trek. Demo punya halaman sendiri, dibuka lewat tombol Demo di header.",
    "tour.lang.title": "Bahasa & tema",
    "tour.lang.body": "Gunakan tombol ini untuk berganti bahasa (Indonesia/Inggris) dan tema (terang/gelap). Pilihan Anda diingat.",
    "noscript": "Dashboard ini membutuhkan JavaScript. API tetap bisa dibaca di",
  },
};

const storedLang = (() => {
  try {
    const saved = localStorage.getItem("sg-lang");
    if (saved === "en" || saved === "id") return saved;
  } catch {}
  return null;
})();

let lang = storedLang ?? ((navigator.language || "").toLowerCase().startsWith("id") ? "id" : "en");

function storeLang(value) {
  try {
    localStorage.setItem("sg-lang", value);
  } catch {}
}

function t(key, vars) {
  let s = I18N[lang]?.[key] ?? I18N.en[key] ?? key;
  if (vars) for (const [k, v] of Object.entries(vars)) s = s.replaceAll(`{${k}}`, v);
  return s;
}

function applyLanguage() {
  document.documentElement.lang = lang;
  document.title = t("page.title");
  document.querySelectorAll("[data-i18n]").forEach((el) => {
    el.textContent = t(el.dataset.i18n);
  });
  document.querySelectorAll("[data-i18n-aria]").forEach((el) => {
    el.setAttribute("aria-label", t(el.dataset.i18nAria));
  });
  document.querySelectorAll("[data-i18n-title]").forEach((el) => {
    el.title = t(el.dataset.i18nTitle);
  });
  const btn = $("#lang-btn");
  btn.textContent = t(lang === "en" ? "lang.btn.toId" : "lang.btn.toEn");
  btn.title = t("lang.switch");
  btn.lang = lang === "en" ? "id" : "en";
  refreshThemeButton();
  render();
}

function currentTheme() {
  return document.documentElement.dataset.theme === "light" ? "light" : "dark";
}

function refreshThemeButton() {
  const btn = $("#theme-btn");
  if (!btn) return;
  const dark = currentTheme() === "dark";
  btn.textContent = t(dark ? "theme.light" : "theme.dark");
  btn.title = t(dark ? "theme.light.title" : "theme.dark.title");
}

function applyTheme(theme) {
  document.documentElement.dataset.theme = theme;
  const meta = document.querySelector('meta[name="color-scheme"]');
  if (meta) meta.content = theme;
  try {
    localStorage.setItem("sg-theme", theme);
  } catch {}
  refreshThemeButton();
  render();
}

const cssVar = (name) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();

const fmt = (v, digits = 2, unit = "") =>
  v === null || v === undefined || !Number.isFinite(v) ? "—" : `${v.toFixed(digits)}${unit}`;
const signed = (v, digits = 2, unit = "") =>
  v === null || v === undefined || !Number.isFinite(v)
    ? "—"
    : `${v >= 0 ? "+" : "−"}${Math.abs(v).toFixed(digits)}${unit}`;

const locale = () => (lang === "id" ? "id-ID" : "en-US");
const clock = (t) =>
  new Date(t * 1000).toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit", second: "2-digit" });
const hhmm = (t) => new Date(t * 1000).toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit" });
const ago = (s) => {
  if (s === null || s === undefined) return "—";
  if (s < 60) return `${Math.max(0, Math.round(s))} s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${Math.round(s % 60)}s`;
  return `${Math.floor(s / 3600)}h ${Math.round((s % 3600) / 60)}m`;
};

function movePoints() {
  return state.data?.series?.movement?.points ?? [];
}
function refPoints() {
  return state.data?.series?.reference?.points ?? [];
}
function activeIndex() {
  const n = movePoints().length;
  if (!n) return -1;
  return state.cursor === null ? n - 1 : clamp(state.cursor, 0, n - 1);
}

/* ---------------- verdict + gauges ---------------- */

function renderVerdict() {
  const d = state.data;
  const rule = d?.rule;
  const st = state.error && !d ? "UNKNOWN" : (rule?.state ?? "LOADING");
  document.body.dataset.state = st;

  $("#state-word").textContent = st === "LOADING" ? "—" : t(`state.${st}`);
  let sentence = st === "LOADING" ? t("verdict.loading") : t(`sentence.${st}`);
  if (state.error && !d) sentence = t("verdict.apiDown");
  else if (st === "UNKNOWN" && rule?.reasons?.includes("DB_UNAVAILABLE"))
    sentence = t("verdict.dbMissing", { path: d?.db?.path ?? "data/raw/slopeguard.db" });
  else if (st === "UNKNOWN" && rule?.reasons?.includes("NO_DATA"))
    sentence = t("verdict.noData");
  $("#sentence").textContent = sentence;

  const nodes = d?.nodes ?? [];
  const stale = $("#stale");
  const scene = document.querySelector(".scene");
  const faulted = nodes.find((n) => n.sensor_status && n.sensor_status !== "OK");
  if (!nodes.length) {
    stale.hidden = false;
    stale.textContent = t("stale.none");
  } else if (faulted) {
    stale.hidden = false;
    stale.textContent = t("stale.fault", { node: faulted.node_id, status: faulted.sensor_status });
  } else if (rule?.quality !== "GOOD") {
    const worst = Math.max(...nodes.map((n) => n.age_s ?? 0));
    stale.hidden = false;
    stale.textContent = t("stale.stale", { age: ago(worst) });
  } else {
    stale.hidden = true;
  }
  scene.classList.toggle("is-stale", !stale.hidden);

  // Sparse: only explain an abnormal verdict; "no abnormal signal" says nothing.
  const codes = (rule?.reasons ?? []).filter((code) => code !== "NO_ABNORMAL_SIGNAL");
  const reasons = $("#reasons");
  reasons.hidden = !codes.length;
  reasons.replaceChildren(
    ...codes.map((code) => {
      const li = document.createElement("li");
      li.textContent = t(`reason.${code}`);
      li.title = code;
      return li;
    }),
  );

  const pi = activeIndex();
  const p = pi >= 0 ? movePoints()[pi] : null;
  const rel = p ? p[3] : rule?.inputs?.rel_pitch_deg;
  $("#g-rel").textContent = signed(rel, 2, "°");
  $("#g-rain").textContent = p ? fmt(sumRain(pi), 1, " mm") : fmt(rule?.inputs?.rain_15m_mm, 1, " mm");
  $("#g-soil").textContent = p ? fmt(p[6], 0, " %") : fmt(rule?.inputs?.soil_moisture_pct, 0, " %");

  let rate = rule?.inputs?.tilt_rate_deg_min;
  const pts = movePoints();
  if (state.cursor !== null && pi >= 0) {
    const back = Math.max(0, pi - 30);
    const a = pts[pi]?.[3];
    const b = pts[back]?.[3];
    rate = a !== null && b !== null && pi - back > 0 ? (a - b) / ((pts[pi][0] - pts[back][0]) / 60) : null;
  }
  $("#g-rate").textContent = rate === null || rate === undefined ? "—" : signed(rate, 2, t("unit.rate"));

  $("#site").textContent = d?.nodes?.[0]?.site_id ?? t("site.none");
  // Sparse: quality is only interesting when it is not good.
  $("#quality").textContent = rule?.quality && rule.quality !== "GOOD" ? t(`quality.${rule.quality}`) : "";
}

function sumRain(idx) {
  const pts = movePoints();
  if (idx < 0) return null;
  const end = pts[idx][0];
  let sum = 0;
  for (let i = idx; i >= 0 && pts[i][0] > end - 900; i--) sum += pts[i][5] ?? 0;
  return sum;
}

/* ---------------- scene ---------------- */

const RAIN_POOL = 72;
const rainDrops = [];
const rain = { raf: 0, last: 0, count: 0, speed: 320 };

function newDrop(el) {
  return {
    el,
    x: Math.random() * 940 - 20,
    y: Math.random() * 660 - 40,
    len: 14 + Math.random() * 16,
    mul: 0.8 + Math.random() * 0.45,
    op: 0.35 + Math.random() * 0.45,
  };
}

function buildRain() {
  const g = $("#rain");
  const frag = document.createDocumentFragment();
  for (let i = 0; i < RAIN_POOL; i++) {
    const el = document.createElementNS("http://www.w3.org/2000/svg", "line");
    el.setAttribute("opacity", (0.35 + Math.random() * 0.45).toFixed(2));
    el.style.display = "none";
    rainDrops.push(newDrop(el));
    frag.append(el);
  }
  g.append(frag);
}

function drawDrops() {
  for (let i = 0; i < RAIN_POOL; i++) {
    const d = rainDrops[i];
    if (i >= rain.count) {
      d.el.style.display = "none";
      continue;
    }
    d.el.style.display = "";
    const slant = d.len * 0.16;
    d.el.setAttribute("x1", d.x.toFixed(1));
    d.el.setAttribute("y1", d.y.toFixed(1));
    d.el.setAttribute("x2", (d.x - slant).toFixed(1));
    d.el.setAttribute("y2", (d.y + d.len).toFixed(1));
  }
}

function rainFrame(now) {
  const dt = Math.min(0.05, Math.max(0, (now - rain.last) / 1000));
  rain.last = now;
  for (let i = 0; i < rain.count; i++) {
    const d = rainDrops[i];
    d.y += rain.speed * d.mul * dt;
    d.x -= rain.speed * 0.09 * d.mul * dt;
    if (d.y > 650) {
      d.y = -30 - Math.random() * 140;
      d.x = Math.random() * 940 - 20;
      d.len = 14 + Math.random() * 16;
      d.mul = 0.8 + Math.random() * 0.45;
    }
    if (d.x < -30) d.x = 940 + Math.random() * 20;
  }
  drawDrops();
  rain.raf = requestAnimationFrame(rainFrame);
}

function stopRain() {
  if (rain.raf) cancelAnimationFrame(rain.raf);
  rain.raf = 0;
  rain.count = 0;
  for (const d of rainDrops) d.el.style.display = "none";
}

// Fall speed scales with the rain rate measured by the sensor: ~4.5 m/s in
// drizzle to ~9 m/s in downpour (scene units are illustrative, not metric).
function setRain(mm15) {
  const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const mmMin = Math.max(0, mm15 ?? 0) / 15;
  const on = mmMin > 0.05 && !reduced;
  $("#rain").classList.toggle("on", on);
  if (!on) return stopRain();
  rain.speed = 315 + 315 * clamp(mmMin / 2, 0, 1);
  rain.count = Math.round(8 + 52 * clamp(mmMin / 1.5, 0, 1));
  if (!rain.raf) {
    rain.last = performance.now();
    drawDrops();
    rain.raf = requestAnimationFrame(rainFrame);
  }
}

function fitScene() {
  const el = $("#slope");
  const rect = el.getBoundingClientRect();
  if (!rect.width || !rect.height) return;
  const aspect = rect.width / rect.height;
  const h = 620;
  const w = Math.min(900, h * aspect);
  const x = clamp(560 - w * 0.62, 0, 900 - w);
  el.setAttribute("viewBox", `${x.toFixed(0)} 0 ${w.toFixed(0)} ${h}`);
}

function renderScene() {
  const pts = movePoints();
  const pi = activeIndex();
  const p = pi >= 0 ? pts[pi] : null;
  const rule = state.data?.rule;
  const rel = p ? (p[3] ?? 0) : (rule?.inputs?.rel_pitch_deg ?? 0);
  const angle = clamp(rel * TILT_DRAW, -22, 22);

  // Pivot at the mast foot (fill-box bottom centre), not the view-box centre.
  $("#move-mast").style.transform = `rotate(${angle.toFixed(2)}deg)`;
  $("#move-angle").textContent = `${rel >= 0 ? "+" : "−"}${Math.abs(rel).toFixed(2)}°`;

  const arcR = 56;
  const a1 = (-90 * Math.PI) / 180;
  const a2 = ((-90 + angle) * Math.PI) / 180;
  const arc = `M ${600 + arcR * Math.cos(a1)} ${374 + arcR * Math.sin(a1)} A ${arcR} ${arcR} 0 0 ${angle >= 0 ? 1 : 0} ${600 + arcR * Math.cos(a2)} ${374 + arcR * Math.sin(a2)}`;
  $("#angle-arc").setAttribute("d", angle === 0 ? "" : arc);

  $("#move-glow").setAttribute("opacity", state.data?.rule?.state === "STABLE" ? "0.25" : "0.9");
  $("#ghost").setAttribute("opacity", Math.abs(rel) > 0.2 ? "0.5" : "0");

  const soil = p ? p[6] : rule?.inputs?.soil_moisture_pct;
  if (soil !== null && soil !== undefined) {
    const y = clamp(560 - soil * 2.6, 300, 540);
    $("#water").setAttribute("d", `M0 ${y} C 220 ${y - 8} 420 ${y + 22} 900 ${y + 112}`);
    $("#water-label").setAttribute("y", String(y + 36));
  }

  const rain15 = p ? sumRain(pi) : rule?.inputs?.rain_15m_mm;
  setRain(rain15);
}

/* ---------------- charts ---------------- */

function setupCanvas(canvas) {
  const dpr = Math.min(2, window.devicePixelRatio || 1);
  const w = canvas.clientWidth || canvas.parentElement.clientWidth;
  const h = canvas.clientHeight || 120;
  if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
  }
  const ctx = canvas.getContext("2d");
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);
  return { ctx, w, h };
}

const MONO = '10px ui-monospace, "SF Mono", Menlo, Consolas, monospace';

function xDomain() {
  const all = [...movePoints(), ...refPoints()];
  if (!all.length) return [0, 1];
  let lo = Infinity;
  let hi = -Infinity;
  for (const p of all) {
    if (p[0] < lo) lo = p[0];
    if (p[0] > hi) hi = p[0];
  }
  if (hi - lo < 60) {
    hi += 30;
    lo -= 30;
  }
  return [lo, hi];
}

function drawTilt() {
  const canvas = $("#chart-tilt");
  const { ctx, w, h } = setupCanvas(canvas);
  const sig = getComputedStyle(document.body).getPropertyValue("--sig").trim() || "#ef5f3c";
  const pts = movePoints();
  const [t0, t1] = xDomain();
  const pad = { l: 34, r: 10, t: 12, b: 20 };
  const iw = Math.max(1, w - pad.l - pad.r);
  const ih = Math.max(1, h - pad.t - pad.b);
  const X = (t) => pad.l + ((t - t0) / (t1 - t0)) * iw;

  let maxAbs = LEVEL_DEG;
  for (const p of pts) if (p[3] !== null) maxAbs = Math.max(maxAbs, Math.abs(p[3]));
  const top = maxAbs * 1.25;
  const Y = (v) => pad.t + ((top - v) / (2 * top)) * ih;

  ctx.strokeStyle = cssVar("--chart-grid");
  ctx.lineWidth = 1;
  for (const v of [-top, -LEVEL_DEG, 0, LEVEL_DEG, top]) {
    if (Math.abs(v) === top) continue;
    ctx.setLineDash(v === 0 ? [] : [3, 4]);
    ctx.strokeStyle = v === 0 ? cssVar("--chart-zero") : v === LEVEL_DEG ? cssVar("--chart-threshold") : cssVar("--chart-grid");
    ctx.beginPath();
    ctx.moveTo(pad.l, Y(v));
    ctx.lineTo(w - pad.r, Y(v));
    ctx.stroke();
  }
  ctx.setLineDash([]);

  ctx.fillStyle = cssVar("--dim");
  ctx.font = MONO;
  ctx.textAlign = "right";
  ctx.fillText("+2°", pad.l - 6, Y(LEVEL_DEG) + 3);
  ctx.fillText("0", pad.l - 6, Y(0) + 3);
  ctx.fillText("−2°", pad.l - 6, Y(-LEVEL_DEG) + 3);

  if (pts.length > 1) {
    const valid = pts.filter((p) => p[3] !== null);
    if (valid.length > 1) {
      ctx.beginPath();
      ctx.moveTo(X(valid[0][0]), Y(0));
      for (const p of valid) ctx.lineTo(X(p[0]), Y(p[3]));
      ctx.lineTo(X(valid[valid.length - 1][0]), Y(0));
      ctx.closePath();
      ctx.globalAlpha = 0.14;
      ctx.fillStyle = sig;
      ctx.fill();
      ctx.globalAlpha = 1;

      ctx.beginPath();
      valid.forEach((p, i) => (i ? ctx.lineTo(X(p[0]), Y(p[3])) : ctx.moveTo(X(p[0]), Y(p[3]))));
      ctx.strokeStyle = sig;
      ctx.lineWidth = 1.6;
      ctx.lineJoin = "round";
      ctx.stroke();
    }
  }

  const pi = activeIndex();
  if (pi >= 0) {
    const p = pts[pi];
    const t = p[0];
    ctx.strokeStyle = cssVar("--chart-cursor");
    ctx.setLineDash([2, 3]);
    ctx.beginPath();
    ctx.moveTo(X(t), pad.t);
    ctx.lineTo(X(t), pad.t + ih);
    ctx.stroke();
    ctx.setLineDash([]);
    if (p[3] !== null) {
      ctx.beginPath();
      ctx.arc(X(t), Y(p[3]), 3.2, 0, Math.PI * 2);
      ctx.fillStyle = cssVar("--chart-dot");
      ctx.fill();
    }
  }

  if (!pts.length) {
    ctx.fillStyle = cssVar("--dim");
    ctx.textAlign = "center";
    ctx.font = '11px system-ui, sans-serif';
    ctx.fillText(t("chart.noPackets"), w / 2, h / 2);
  }

  ctx.fillStyle = cssVar("--dim");
  ctx.font = MONO;
  ctx.textAlign = "left";
  ctx.fillText(hhmm(t0), pad.l, h - 6);
  ctx.textAlign = "right";
  ctx.fillText(hhmm(t1), w - pad.r, h - 6);
}

function drawLoad() {
  const canvas = $("#chart-load");
  const { ctx, w, h } = setupCanvas(canvas);
  const pts = movePoints();
  const [t0, t1] = xDomain();
  const pad = { l: 34, r: 34, t: 10, b: 20 };
  const iw = Math.max(1, w - pad.l - pad.r);
  const ih = Math.max(1, h - pad.t - pad.b);
  const X = (t) => pad.l + ((t - t0) / (t1 - t0)) * iw;
  const Ys = (v) => pad.t + ((100 - v) / 100) * ih;

  let rainMax = 0.05;
  for (const p of pts) rainMax = Math.max(rainMax, p[5] ?? 0);
  const Yr = (v) => pad.t + ih - (v / rainMax) * ih;

  ctx.strokeStyle = cssVar("--chart-grid");
  ctx.lineWidth = 1;
  for (const v of [0, 50, 100]) {
    ctx.beginPath();
    ctx.moveTo(pad.l, Ys(v));
    ctx.lineTo(w - pad.r, Ys(v));
    ctx.stroke();
  }
  ctx.fillStyle = cssVar("--dim");
  ctx.font = MONO;
  ctx.textAlign = "right";
  ctx.fillText("100", pad.l - 6, Ys(100) + 3);
  ctx.fillText("0", pad.l - 6, Ys(0) + 3);
  ctx.textAlign = "left";
  ctx.fillText(t("chart.soil"), w - pad.r + 6, Ys(100) + 3);

  const barW = Math.max(1, (iw / Math.max(1, pts.length)) * 0.55);
  ctx.fillStyle = cssVar("--chart-rain");
  for (const p of pts) {
    const v = p[5];
    if (!v) continue;
    const y = Yr(v);
    ctx.fillRect(X(p[0]) - barW / 2, y, barW, pad.t + ih - y);
  }

  const soilPts = pts.filter((p) => p[6] !== null);
  if (soilPts.length > 1) {
    ctx.beginPath();
    soilPts.forEach((p, i) => (i ? ctx.lineTo(X(p[0]), Ys(p[6])) : ctx.moveTo(X(p[0]), Ys(p[6]))));
    ctx.strokeStyle = cssVar("--chart-soil");
    ctx.lineWidth = 1.4;
    ctx.stroke();
  }

  const pi = activeIndex();
  if (pi >= 0) {
    ctx.strokeStyle = cssVar("--chart-cursor");
    ctx.setLineDash([2, 3]);
    ctx.beginPath();
    ctx.moveTo(X(pts[pi][0]), pad.t);
    ctx.lineTo(X(pts[pi][0]), pad.t + ih);
    ctx.stroke();
    ctx.setLineDash([]);
  }

  ctx.fillStyle = cssVar("--dim");
  ctx.font = MONO;
  ctx.textAlign = "left";
  ctx.fillText(hhmm(t0), pad.l, h - 6);
  ctx.textAlign = "right";
  ctx.fillText(hhmm(t1), w - pad.r, h - 6);

  $("#load-note").innerHTML = pts.length
    ? t("chart.bars", {
        peak: rainMax.toFixed(2),
        from: fmt(soilPts[0]?.[6], 0, "%"),
        to: fmt(soilPts[soilPts.length - 1]?.[6], 0, "%"),
      })
    : t("load.empty");
}

function renderNotes() {
  const d = state.data;
  if (!d) {
    $("#tilt-note").textContent = t("trend.empty");
    return;
  }
  const rel = d.rule?.inputs?.rel_pitch_deg;
  const thr = d.rule?.thresholds?.rel_warn_deg ?? LEVEL_DEG;
  const n = d.rule?.inputs?.sustained_windows ?? 0;
  $("#tilt-note").innerHTML = t(n === 1 ? "chart.baseline1" : "chart.baseline", {
    thr: thr.toFixed(1),
    rel: signed(rel, 2, "°"),
    n,
  });
}

/* ---------------- trust + strip ---------------- */

function renderTrust() {
  const d = state.data;
  const tbody = $("#nodes");
  const rows = [...(d?.nodes ?? [])].sort((a, b) => (a.node_role === "movement" ? -1 : b.node_role === "movement" ? 1 : 0));
  tbody.replaceChildren(
    ...(rows.length
      ? rows.map((n) => {
          const tr = document.createElement("tr");
          tr.dataset.role = n.node_role;
          const stale = n.age_s !== null && n.age_s > 30;
          tr.innerHTML = `
            <td>${t(`role.${n.node_role}`)}<br><span class="mono" style="color:var(--dim)">${n.node_id}</span></td>
            <td>${n.seq}</td>
            <td class="${stale ? "age-stale" : ""}">${ago(n.age_s)}</td>
            <td>${fmt(n.battery_v, 2)}</td>
            <td>${n.signal_dbm ?? "—"}</td>`;
          return tr;
        })
      : [Object.assign(document.createElement("tr"), {
          innerHTML: `<td colspan="5" class="empty">${t("nodes.empty")}</td>`,
        })])
  );

  const model = d?.model;
  $("#fact-model").textContent = model?.available
    ? t("model.live")
    : model?.report_present
      ? t("model.report")
      : t("model.none");
  $("#fact-rule").textContent = d?.rule?.basis === "evidence-rule" ? t("facts.rule.value") : (d?.rule?.basis ?? "—");
  $("#fact-ingest").textContent = d?.receiver?.up ? t("ingest.up") : t("ingest.down");
  $("#fact-ingest").style.color = d?.receiver?.up ? "" : "var(--bad)";
  const rejects = d?.db?.rejects;
  $("#fact-rejects").textContent = rejects === undefined ? "—" : `${rejects}${d.db.last_reject ? ` · ${String(d.db.last_reject.reason).slice(0, 22)}` : ""}`;
  $("#fact-rejects").title = d?.db?.last_reject ? `${d.db.last_reject.at} — ${d.db.last_reject.reason}` : t("facts.rejects.none");
}

function renderStrip() {
  const pts = movePoints();
  const scrub = $("#scrub");
  const n = pts.length;
  scrub.max = String(Math.max(0, n - 1));
  scrub.disabled = n < 2;
  const i = activeIndex();
  scrub.value = String(Math.max(0, i));
  $("#cursor-label").textContent =
    state.cursor === null ? t("strip.liveEdge") : t("strip.viewing", { time: clock(pts[i][0]) });
  $("#cursor-label").classList.toggle("at", state.cursor !== null);
  document.querySelectorAll(".mark").forEach((m) => (m.hidden = true));

  if (n > 1) {
    const [t0, t1] = xDomain();
    const pct = (time) => clamp(((time - t0) / (t1 - t0)) * 100, 0, 100);
    const firstRain = pts.find((p) => (p[5] ?? 0) > 0.01);
    const firstTilt = pts.find((p) => (p[3] ?? 0) >= LEVEL_DEG);
    if (firstRain) {
      const m = $("#mark-rain");
      m.hidden = false;
      m.style.left = `${pct(firstRain[0])}%`;
      m.title = t("mark.rain", { time: clock(firstRain[0]) });
    }
    if (firstTilt) {
      const m = $("#mark-tilt");
      m.hidden = false;
      m.style.left = `${pct(firstTilt[0])}%`;
      m.title = t("mark.tilt", { time: clock(firstTilt[0]) });
    }
    $("#mark-key").hidden = !(firstRain || firstTilt);
    $("#t-start").textContent = clock(t0);
    $("#t-end").textContent = clock(t1);
  } else {
    $("#t-start").textContent = "—";
    $("#t-end").textContent = "—";
    $("#mark-key").hidden = true;
  }
  // Sparse: the Live button only exists when you have left live.
  $("#live-btn").hidden = state.cursor === null;
  $("#clock").textContent = n ? clock(pts[Math.max(0, i)][0]) : "--:--:--";
  $("#clock").title = t("clock.title");
}

/* ---------------- render ---------------- */

function render() {
  fitScene();
  renderVerdict();
  renderScene();
  drawTilt();
  drawLoad();
  renderNotes();
  renderTrust();
  renderStrip();
  const up = state.data?.receiver?.up;
  $("#ingest-dot").className = `dot ${up ? "on" : "off"}`;
  $("#ingest-text").textContent = up ? t("ingest.up") : t("ingest.down");
  $("#ingest-text").hidden = !!up;
  $("#ingest").title = up ? t("ingest.up") : t("ingest.down");
}

/* ---------------- data ---------------- */

let fetching = false;
async function refresh() {
  if (fetching) return;
  fetching = true;
  try {
    const res = await fetch(`/api/data?window_s=${WINDOW_S}`, { cache: "no-store" });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const next = await res.json();
    if (state.cursor !== null) {
      const n = next?.series?.movement?.points?.length ?? 0;
      state.cursor = n ? clamp(state.cursor, 0, n - 1) : null;
    }
    state.data = next;
    state.error = null;
  } catch (err) {
    state.error = err.message;
  } finally {
    fetching = false;
    render();
  }
}

/* ---------------- actions ---------------- */

function stepCursor(delta) {
  const n = movePoints().length;
  if (!n) return;
  const cur = activeIndex();
  state.cursor = clamp(cur + delta, 0, n - 1);
  render();
}

function goLive() {
  state.cursor = null;
  render();
}

/* ---------------- guided tour ---------------- */

const TOUR = [
  { sel: ".verdict", key: "verdict" },
  { sel: "#slope", key: "scene" },
  { sel: ".gauges", key: "gauges" },
  { sel: "#evidence", key: "evidence" },
  { sel: "details.more", key: "more", open: true },
  { sel: ".strip", key: "strip" },
  { sel: ".bar-meta", key: "lang" },
];

let tourIndex = 0;
let tourTarget = null;

function startTour() {
  if (!$("#tour").hidden) return;
  tourIndex = 0;
  $("#tour").hidden = false;
  document.querySelector(".bar").inert = true;
  document.querySelector(".shell").inert = true;
  document.body.classList.add("touring");
  showTourStep();
}

function showTourStep() {
  const step = TOUR[tourIndex];
  const el = document.querySelector(step.sel);
  tourTarget = el;
  if (step.open) el.open = true;
  el.scrollIntoView({ block: "center", inline: "nearest" });
  $("#tour-title").textContent = t(`tour.${step.key}.title`);
  $("#tour-body").textContent = t(`tour.${step.key}.body`);
  $("#tour-count").textContent = t("tour.count", { n: tourIndex + 1, total: TOUR.length });
  $("#tour-prev").disabled = tourIndex === 0;
  $("#tour-prev").textContent = t("tour.back");
  $("#tour-skip").textContent = t("tour.skip");
  $("#tour-next").textContent = tourIndex === TOUR.length - 1 ? t("tour.done") : t("tour.next");
  requestAnimationFrame(() => positionTour(el));
  $("#tour-next").focus({ preventScroll: true });
}

function positionTour(el) {
  if (!el) return;
  const pad = 8;
  const r = el.getBoundingClientRect();
  const spot = $("#tour-spot");
  spot.style.left = `${Math.max(0, r.left - pad)}px`;
  spot.style.top = `${Math.max(0, r.top - pad)}px`;
  spot.style.width = `${Math.min(window.innerWidth, r.width + pad * 2)}px`;
  spot.style.height = `${r.height + pad * 2}px`;
  $("#tour-panel").classList.toggle("top", r.top + r.height / 2 > window.innerHeight * 0.55);
}

function endTour() {
  $("#tour").hidden = true;
  document.querySelector(".bar").inert = false;
  document.querySelector(".shell").inert = false;
  document.body.classList.remove("touring");
  try {
    localStorage.setItem("sg-toured", "1");
  } catch {}
  tourTarget = null;
  $("#help-btn")?.focus({ preventScroll: true });
}

function tourNext() {
  if (tourIndex >= TOUR.length - 1) return endTour();
  tourIndex += 1;
  showTourStep();
}

function tourPrev() {
  if (tourIndex === 0) return;
  tourIndex -= 1;
  showTourStep();
}

function tourDone() {
  try {
    return localStorage.getItem("sg-toured") === "1";
  } catch {
    return true;
  }
}

function autoTour() {
  if (!tourDone()) setTimeout(startTour, 700);
}

/* ---------------- wiring ---------------- */

function wire() {
  buildRain();

  $("#scrub").addEventListener("input", (e) => {
    state.cursor = Number(e.target.value);
    render();
  });

  $("#live-btn").addEventListener("click", goLive);

  $("#theme-btn").addEventListener("click", () => {
    applyTheme(currentTheme() === "dark" ? "light" : "dark");
  });

  $("#lang-btn").addEventListener("click", () => {
    lang = lang === "en" ? "id" : "en";
    storeLang(lang);
    applyLanguage();
  });

  const langPick = $("#lang-pick");
  langPick.querySelectorAll("[data-lang]").forEach((btn) => {
    btn.addEventListener("click", () => {
      lang = btn.dataset.lang;
      storeLang(lang);
      langPick.close();
      applyLanguage();
      autoTour();
    });
  });

  const help = $("#help");
  $("#help-btn").addEventListener("click", () => help.showModal());
  $("#help-close").addEventListener("click", () => help.close());
  $("#tour-btn").addEventListener("click", () => {
    help.close();
    startTour();
  });
  $("#tour-next").addEventListener("click", tourNext);
  $("#tour-prev").addEventListener("click", tourPrev);
  $("#tour-skip").addEventListener("click", endTour);
  help.addEventListener("click", (e) => {
    if (e.target === help) help.close();
  });

  document.addEventListener("keydown", (e) => {
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const tag = e.target.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;
    if (!$("#tour").hidden) {
      if (e.key === "Escape") {
        e.preventDefault();
        endTour();
      } else if (e.key === "ArrowRight") {
        e.preventDefault();
        tourNext();
      } else if (e.key === "ArrowLeft") {
        e.preventDefault();
        tourPrev();
      }
      return;
    }
    if (langPick.open) return;
    if (help.open && e.key !== "Escape" && e.key !== "?") return;
    switch (e.key) {
      case "?":
        e.preventDefault();
        help.open ? help.close() : help.showModal();
        break;
      case "l":
      case "L":
        goLive();
        break;
      case "ArrowLeft":
        e.preventDefault();
        stepCursor(-1);
        break;
      case "ArrowRight":
        e.preventDefault();
        stepCursor(1);
        break;
      default:
        break;
    }
  });

  window.addEventListener("resize", render);
  window.addEventListener("resize", () => {
    if (!$("#tour").hidden) positionTour(tourTarget);
  });
  window.addEventListener(
    "scroll",
    () => {
      if (!$("#tour").hidden) positionTour(tourTarget);
    },
    true,
  );
  document.addEventListener("visibilitychange", () => {
    if (!document.hidden) refresh();
  });

  applyLanguage();
  const needsTour = !tourDone();
  $("#lang-tour-note").hidden = !needsTour;
  if (!storedLang || needsTour) {
    langPick.querySelectorAll("[data-lang]").forEach((btn) => {
      const current = btn.dataset.lang === lang;
      btn.classList.toggle("is-current", current);
      btn.setAttribute("aria-pressed", String(current));
    });
    langPick.showModal();
  } else {
    autoTour();
  }
  setInterval(() => {
    if (!document.hidden) refresh();
  }, 2000);
  refresh();
}

wire();
