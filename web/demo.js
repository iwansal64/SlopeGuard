// SlopeGuard demo page. Plays recorded scenarios into the same local store.
const $ = (sel) => document.querySelector(sel);

const I18N = {
  en: {
    "demo.pageTitle": "SlopeGuard — demo mode",
    "demo.brand": "demo mode",
    "demo.back": "Live dashboard",
    "demo.kicker": "Synthetic recordings",
    "demo.title": "Demo mode",
    "demo.intro": "Play a recorded slope scenario into this local dashboard to see how SlopeGuard reacts. Nothing on this page is live field data.",
    "demo.storm": "Storm demo",
    "demo.dry": "Calm demo",
    "demo.idle": "Choose a demo to play.",
    "demo.storm.desc": "Heavy rain, rising soil moisture, and tilt crossing the 2° baseline.",
    "demo.dry.desc": "A calm day: the slope stays inside the baseline.",
    "demo.playing": "{label}: playing the recording…",
    "demo.done": "{label} finished: {accepted} new packets loaded.",
    "demo.dup": "This demo was already loaded. Reset the demo to watch it again.",
    "demo.fail": "Demo could not start: {error}",
    "demo.note": "Demos are recordings, not live data. Their packets join this dashboard's 30-minute history and then age out. The live dashboard always shows whatever the sensors send.",
    "demo.open": "Open the live dashboard",
    "demo.ops": "For operators",
    "demo.ops.body": "To remove demo packets immediately, stop the services and run",
    "demo.noscript": "This page needs JavaScript.",
    "demo.label.storm": "Storm demo",
    "demo.label.dry": "Calm demo",
    "theme.light": "Light",
    "theme.dark": "Dark",
    "theme.light.title": "Switch to light theme",
    "theme.dark.title": "Switch to dark theme",
    "lang.title": "Switch to Bahasa Indonesia",
    "lang.toId": "Bahasa",
    "lang.toEn": "English",
  },
  id: {
    "demo.pageTitle": "SlopeGuard — mode demo",
    "demo.brand": "mode demo",
    "demo.back": "Dashboard live",
    "demo.kicker": "Rekaman sintetis",
    "demo.title": "Mode demo",
    "demo.intro": "Putar skenario lereng yang sudah direkam ke dashboard lokal ini untuk melihat reaksi SlopeGuard. Tidak ada data lapangan langsung di halaman ini.",
    "demo.storm": "Demo badai",
    "demo.dry": "Demo tenang",
    "demo.idle": "Pilih demo untuk diputar.",
    "demo.storm.desc": "Hujan lebat, kelembapan tanah naik, dan tilt melewati baseline 2°.",
    "demo.dry.desc": "Hari tenang: lereng tetap di bawah baseline.",
    "demo.playing": "{label}: memutar rekaman…",
    "demo.done": "{label} selesai: {accepted} paket baru dimuat.",
    "demo.dup": "Demo ini sudah pernah dimuat. Reset demo untuk menontonnya lagi.",
    "demo.fail": "Demo tidak bisa dimulai: {error}",
    "demo.note": "Demo adalah rekaman, bukan data langsung. Paketnya masuk ke riwayat 30 menit dashboard ini lalu hilang seiring waktu. Dashboard live selalu menampilkan data yang dikirim sensor.",
    "demo.open": "Buka dashboard live",
    "demo.ops": "Untuk operator",
    "demo.ops.body": "Untuk menghapus paket demo segera, hentikan layanan lalu jalankan",
    "demo.noscript": "Halaman ini membutuhkan JavaScript.",
    "demo.label.storm": "Demo badai",
    "demo.label.dry": "Demo tenang",
    "theme.light": "Terang",
    "theme.dark": "Gelap",
    "theme.light.title": "Ganti ke tema terang",
    "theme.dark.title": "Ganti ke tema gelap",
    "lang.title": "Ganti ke Bahasa Inggris",
    "lang.toId": "Bahasa",
    "lang.toEn": "English",
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

function t(key, vars) {
  let s = I18N[lang]?.[key] ?? I18N.en[key] ?? key;
  if (vars) for (const [k, v] of Object.entries(vars)) s = s.replaceAll(`{${k}}`, v);
  return s;
}

function currentTheme() {
  return document.documentElement.dataset.theme === "light" ? "light" : "dark";
}

function refreshThemeButton() {
  const btn = $("#theme-btn");
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
}

function applyLanguage() {
  document.documentElement.lang = lang;
  document.title = t("demo.pageTitle");
  document.querySelectorAll("[data-i18n]").forEach((el) => {
    el.textContent = t(el.dataset.i18n);
  });
  const btn = $("#lang-btn");
  btn.textContent = t(lang === "en" ? "lang.toId" : "lang.toEn");
  btn.title = t("lang.title");
  btn.lang = lang === "en" ? "id" : "en";
  refreshThemeButton();
}

function setStatus(message, isError = false) {
  const el = $("#demo-status");
  el.textContent = message;
  el.classList.toggle("err", isError);
}

let busy = false;
async function play(scenario, labelKey, btn) {
  if (busy) return;
  busy = true;
  const buttons = [$("#storm-btn"), $("#dry-btn")];
  buttons.forEach((b) => (b.disabled = true));
  const label = t(labelKey);
  setStatus(t("demo.playing", { label }));
  try {
    const res = await fetch("/api/replay", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ scenario, rate: 8 }),
    });
    const body = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error(body.detail || body.error || `HTTP ${res.status}`);
    if (body.accepted > 0) setStatus(t("demo.done", { label, accepted: body.accepted }));
    else if (body.duplicate > 0) setStatus(t("demo.dup"));
    else setStatus(t("demo.fail", { error: body.errors?.[0] ?? "unknown" }), true);
  } catch (err) {
    setStatus(t("demo.fail", { error: err.message }), true);
  } finally {
    busy = false;
    buttons.forEach((b) => (b.disabled = false));
  }
}

$("#theme-btn").addEventListener("click", () => {
  applyTheme(currentTheme() === "dark" ? "light" : "dark");
});

$("#lang-btn").addEventListener("click", () => {
  lang = lang === "en" ? "id" : "en";
  try {
    localStorage.setItem("sg-lang", lang);
  } catch {}
  applyLanguage();
});

$("#storm-btn").addEventListener("click", (e) => play("storm", "demo.label.storm", e.currentTarget));
$("#dry-btn").addEventListener("click", (e) => play("dry", "demo.label.dry", e.currentTarget));

applyLanguage();
