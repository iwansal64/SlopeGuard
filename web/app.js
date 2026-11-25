// SlopeGuard dashboard. No framework, no build step: one poll, two canvases.
const $ = (sel) => document.querySelector(sel);
const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));

const state = {
  data: null,
  cursor: null, // index into movement points, null = live
  error: null,
  replaying: false,
};

const LEVEL_DEG = 2.0; // mirrors analysis::BaselineConfig::default().relative_pitch_deg
const TILT_DRAW = 4; // visual exaggeration, labelled on the scene
const WINDOW_S = 1800;

const REASON_TEXT = {
  NO_ABNORMAL_SIGNAL: "no abnormal signal",
  REL_TILT_ABOVE_BASELINE: "relative tilt ≥ 2.0°",
  TILT_RATE_RISING: "tilt rate ≥ 0.2°/min",
  RAINFALL_LOADING: "rain ≥ 5 mm / 15 min",
  SOIL_MOISTURE_RISING: "soil moisture rising",
  REFERENCE_STALE: "reference node stale or faulted",
  NO_DATA: "no packets in window",
  DB_UNAVAILABLE: "database unavailable",
};

const SENTENCE = {
  STABLE: "Movement stays inside the baseline. Keep watching.",
  WATCH: "Loading is rising. No confirmed movement yet.",
  MOVEMENT_RISK: "Relative tilt is above baseline and still rising.",
  CRITICAL: "Independent movement evidence recorded.",
  UNKNOWN: "The measurement cannot be trusted right now.",
  LOADING: "Connecting to the site…",
};

const fmt = (v, digits = 2, unit = "") =>
  v === null || v === undefined || !Number.isFinite(v) ? "—" : `${v.toFixed(digits)}${unit}`;
const signed = (v, digits = 2, unit = "") =>
  v === null || v === undefined || !Number.isFinite(v)
    ? "—"
    : `${v >= 0 ? "+" : "−"}${Math.abs(v).toFixed(digits)}${unit}`;

const clock = (t) =>
  new Date(t * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
const hhmm = (t) => new Date(t * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
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
  const st = state.error && !d ? "UNKNOWN" : (rule?.state ?? "UNKNOWN");
  document.body.dataset.state = st;

  $("#state-word").textContent = st.replace("_", " ");
  let sentence = SENTENCE[st] ?? SENTENCE.UNKNOWN;
  if (state.error && !d) sentence = "The dashboard API is unreachable. Retrying every 2 s.";
  else if (st === "UNKNOWN" && rule?.reasons?.includes("DB_UNAVAILABLE"))
    sentence = `No database at ${d?.db?.path ?? "data/raw/slopeguard.db"}. Start the receiver or ingest a fixture.`;
  else if (st === "UNKNOWN" && rule?.reasons?.includes("NO_DATA"))
    sentence = "No telemetry yet. Start the receiver, then replay a fixture.";
  $("#sentence").textContent = sentence;

  const nodes = d?.nodes ?? [];
  const stale = $("#stale");
  const scene = document.querySelector(".scene");
  const faulted = nodes.find((n) => n.sensor_status && n.sensor_status !== "OK");
  if (!nodes.length) {
    stale.hidden = false;
    stale.textContent = "NO TELEMETRY — nothing has reached the receiver yet. Replay a fixture or power the device.";
  } else if (faulted) {
    stale.hidden = false;
    stale.textContent = `SENSOR FAULT — ${faulted.node_id} reports ${faulted.sensor_status}. Measurements are not trusted.`;
  } else if (rule?.quality !== "GOOD") {
    const worst = Math.max(...nodes.map((n) => n.age_s ?? 0));
    stale.hidden = false;
    stale.textContent = `STALE — newest packet ${ago(worst)} old. Values shown are the last known, not current.`;
  } else {
    stale.hidden = true;
  }
  scene.classList.toggle("is-stale", !stale.hidden);

  const reasons = $("#reasons");
  reasons.replaceChildren(
    ...(rule?.reasons ?? []).map((code) => {
      const li = document.createElement("li");
      li.textContent = REASON_TEXT[code] ?? code;
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
  $("#g-rate").textContent = rate === null || rate === undefined ? "—" : signed(rate, 2, "°/min");

  $("#site").textContent = d?.nodes?.[0]?.site_id ?? "no site";
  $("#quality").textContent = rule?.quality ? `data ${rule.quality.toLowerCase()}` : "—";
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

function buildRain() {
  const g = $("#rain");
  const frag = document.createDocumentFragment();
  for (let i = 0; i < 22; i++) {
    const line = document.createElementNS("http://www.w3.org/2000/svg", "line");
    const x = 20 + ((i * 137) % 860);
    const y = 30 + ((i * 53) % 170);
    line.setAttribute("x1", x);
    line.setAttribute("y1", y);
    line.setAttribute("x2", x - 3);
    line.setAttribute("y2", y + 22);
    frag.append(line);
  }
  g.append(frag);
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

  $("#move-mast").setAttribute("transform", `rotate(${angle.toFixed(2)} 600 374)`);
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
  }

  const rain = p ? p[5] : rule?.inputs?.rain_15m_mm ? 0.2 : 0;
  const rainEl = $("#rain");
  const on = (rain ?? 0) > 0.01;
  rainEl.classList.toggle("on", on && !matchMedia("(prefers-reduced-motion: reduce)").matches);
  rainEl.style.animationDuration = on ? `${clamp(1.6 - rain * 4, 0.55, 1.6)}s` : "";
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

  ctx.strokeStyle = "#2b231c";
  ctx.lineWidth = 1;
  for (const v of [-top, -LEVEL_DEG, 0, LEVEL_DEG, top]) {
    if (Math.abs(v) === top) continue;
    ctx.setLineDash(v === 0 ? [] : [3, 4]);
    ctx.strokeStyle = v === 0 ? "#4a3a2d" : v === LEVEL_DEG ? "rgba(239,95,60,0.55)" : "#2b231c";
    ctx.beginPath();
    ctx.moveTo(pad.l, Y(v));
    ctx.lineTo(w - pad.r, Y(v));
    ctx.stroke();
  }
  ctx.setLineDash([]);

  ctx.fillStyle = "#7a6d60";
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
    ctx.strokeStyle = "rgba(242,236,226,0.35)";
    ctx.setLineDash([2, 3]);
    ctx.beginPath();
    ctx.moveTo(X(t), pad.t);
    ctx.lineTo(X(t), pad.t + ih);
    ctx.stroke();
    ctx.setLineDash([]);
    if (p[3] !== null) {
      ctx.beginPath();
      ctx.arc(X(t), Y(p[3]), 3.2, 0, Math.PI * 2);
      ctx.fillStyle = "#f2ece2";
      ctx.fill();
    }
  }

  if (!pts.length) {
    ctx.fillStyle = "#7a6d60";
    ctx.textAlign = "center";
    ctx.font = '11px system-ui, sans-serif';
    ctx.fillText("no movement packets in this window", w / 2, h / 2);
  }

  ctx.fillStyle = "#7a6d60";
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

  ctx.strokeStyle = "#2b231c";
  ctx.lineWidth = 1;
  for (const v of [0, 50, 100]) {
    ctx.beginPath();
    ctx.moveTo(pad.l, Ys(v));
    ctx.lineTo(w - pad.r, Ys(v));
    ctx.stroke();
  }
  ctx.fillStyle = "#7a6d60";
  ctx.font = MONO;
  ctx.textAlign = "right";
  ctx.fillText("100", pad.l - 6, Ys(100) + 3);
  ctx.fillText("0", pad.l - 6, Ys(0) + 3);
  ctx.textAlign = "left";
  ctx.fillText("soil %", w - pad.r + 6, Ys(100) + 3);

  const barW = Math.max(1, (iw / Math.max(1, pts.length)) * 0.55);
  ctx.fillStyle = "rgba(79,143,154,0.55)";
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
    ctx.strokeStyle = "#a19384";
    ctx.lineWidth = 1.4;
    ctx.stroke();
  }

  const pi = activeIndex();
  if (pi >= 0) {
    ctx.strokeStyle = "rgba(242,236,226,0.35)";
    ctx.setLineDash([2, 3]);
    ctx.beginPath();
    ctx.moveTo(X(pts[pi][0]), pad.t);
    ctx.lineTo(X(pts[pi][0]), pad.t + ih);
    ctx.stroke();
    ctx.setLineDash([]);
  }

  ctx.fillStyle = "#7a6d60";
  ctx.font = MONO;
  ctx.textAlign = "left";
  ctx.fillText(hhmm(t0), pad.l, h - 6);
  ctx.textAlign = "right";
  ctx.fillText(hhmm(t1), w - pad.r, h - 6);

  $("#load-note").innerHTML = pts.length
    ? `bars rainfall per 10 s packet (peak <b>${rainMax.toFixed(2)} mm</b>) · line soil <b>${fmt(soilPts[0]?.[6], 0, "%")} → ${fmt(soilPts[soilPts.length - 1]?.[6], 0, "%")}</b>`
    : "no packets in this window";
}

function renderNotes() {
  const d = state.data;
  const rel = d?.rule?.inputs?.rel_pitch_deg;
  const thr = d?.rule?.thresholds?.rel_warn_deg ?? LEVEL_DEG;
  const n = d?.rule?.inputs?.sustained_windows ?? 0;
  $("#tilt-note").innerHTML = d
    ? `baseline <b>${thr.toFixed(1)}°</b> · now <b>${signed(rel, 2, "°")}</b> · ${n} packet${n === 1 ? "" : "s"} above`
    : "";
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
            <td>${n.node_role}<br><span class="mono" style="color:var(--dim)">${n.node_id}</span></td>
            <td>${n.seq}</td>
            <td class="${stale ? "age-stale" : ""}">${ago(n.age_s)}</td>
            <td>${fmt(n.battery_v, 2)}</td>
            <td>${n.signal_dbm ?? "—"}</td>`;
          return tr;
        })
      : [Object.assign(document.createElement("tr"), {
          innerHTML: `<td colspan="5" class="empty">no accepted packets yet</td>`,
        })])
  );

  const model = d?.model;
  $("#fact-model").textContent = model?.available
    ? "live inference"
    : model?.report_present
      ? "report ready, rule shown"
      : "not wired, rule shown";
  $("#fact-rule").textContent = d?.rule?.basis === "evidence-rule" ? "evidence baseline" : (d?.rule?.basis ?? "—");
  $("#fact-ingest").textContent = d?.receiver?.up ? "receiver up" : "receiver down";
  $("#fact-ingest").style.color = d?.receiver?.up ? "" : "#ef5f3c";
  const rejects = d?.db?.rejects;
  $("#fact-rejects").textContent = rejects === undefined ? "—" : `${rejects}${d.db.last_reject ? ` · ${String(d.db.last_reject.reason).slice(0, 22)}` : ""}`;
  $("#fact-rejects").title = d?.db?.last_reject ? `${d.db.last_reject.at} — ${d.db.last_reject.reason}` : "no rejects recorded";
}

function renderStrip() {
  const pts = movePoints();
  const scrub = $("#scrub");
  const n = pts.length;
  scrub.max = String(Math.max(0, n - 1));
  scrub.disabled = n < 2;
  const i = activeIndex();
  scrub.value = String(Math.max(0, i));
  $("#cursor-label").textContent = state.cursor === null ? "LIVE EDGE" : `VIEWING ${clock(pts[i][0])}`;
  $("#cursor-label").classList.toggle("at", state.cursor !== null);
  document.querySelectorAll(".mark").forEach((m) => (m.hidden = true));

  if (n > 1) {
    const [t0, t1] = xDomain();
    const pct = (t) => clamp(((t - t0) / (t1 - t0)) * 100, 0, 100);
    const firstRain = pts.find((p) => (p[5] ?? 0) > 0.01);
    const firstTilt = pts.find((p) => (p[3] ?? 0) >= LEVEL_DEG);
    if (firstRain) {
      const m = $("#mark-rain");
      m.hidden = false;
      m.style.left = `${pct(firstRain[0])}%`;
      m.title = `rain onset ${clock(firstRain[0])}`;
    }
    if (firstTilt) {
      const m = $("#mark-tilt");
      m.hidden = false;
      m.style.left = `${pct(firstTilt[0])}%`;
      m.title = `2° crossed ${clock(firstTilt[0])}`;
    }
    $("#t-start").textContent = clock(t0);
    $("#t-end").textContent = clock(t1);
  } else {
    $("#t-start").textContent = "—";
    $("#t-end").textContent = "—";
  }
  $("#live-btn").disabled = state.cursor === null;
  $("#clock").textContent = n ? clock(pts[Math.max(0, i)][0]) : "--:--:--";
  $("#clock").title = "site clock, latest packet";
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
  $("#ingest-dot").className = `dot ${state.data?.receiver?.up ? "on" : "off"}`;
  $("#ingest-text").textContent = state.data?.receiver?.up ? "receiver up" : "receiver down";
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

let toastTimer;
function toast(message, kind = "") {
  const el = $("#toast");
  el.textContent = message;
  el.className = `toast show ${kind}`;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (el.className = "toast"), 5200);
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function replay(scenario, label, btn) {
  if (state.replaying) return;
  state.replaying = true;
  btn.disabled = true;
  toast(`${label}: pushing the synthetic fixture through the receiver…`);
  try {
    const res = await fetch("/api/replay", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ scenario, rate: 8 }),
    });
    const body = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error(body.detail || body.error || `HTTP ${res.status}`);
    const extra = body.errors?.length ? ` — ${body.errors[0]}` : "";
    const nothingNew = body.pushed > 0 && body.accepted === 0;
    toast(
      `${label}: ${body.pushed} pushed · ${body.accepted} accepted · ${body.duplicate} duplicate · ${body.rejected} rejected${extra}` +
        (nothingNew ? " — this fixture is already stored. Run ./run.sh demo reset for a clean run." : ""),
      body.rejected || body.errors?.length ? "err" : nothingNew ? "warn" : "",
    );
    $("#source-note").hidden = false;
    $("#source-note").textContent =
      "Synthetic fixture pushed through the live receiver. Pipeline demo, not field data.";
    state.cursor = null;
    for (let i = 0; i < 10; i++) {
      await sleep(320);
      await refresh();
    }
  } catch (err) {
    const target = state.data?.receiver?.url ?? "127.0.0.1:8080";
    toast(`Replay failed: ${err.message}. Is slopeguard serve running on ${target}?`, "err");
  } finally {
    state.replaying = false;
    btn.disabled = false;
  }
}

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

/* ---------------- wiring ---------------- */

function wire() {
  buildRain();

  $("#scrub").addEventListener("input", (e) => {
    state.cursor = Number(e.target.value);
    render();
  });

  $("#live-btn").addEventListener("click", goLive);
  $("#storm-btn").addEventListener("click", (e) => replay("storm", "Storm replay", e.currentTarget));
  $("#dry-btn").addEventListener("click", (e) => replay("dry", "Dry control", e.currentTarget));

  const help = $("#help");
  $("#help-btn").addEventListener("click", () => help.showModal());
  $("#help-close").addEventListener("click", () => help.close());
  help.addEventListener("click", (e) => {
    if (e.target === help) help.close();
  });

  document.addEventListener("keydown", (e) => {
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const tag = e.target.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;
    if (help.open && e.key !== "Escape" && e.key !== "?") return;
    switch (e.key) {
      case "?":
        e.preventDefault();
        help.open ? help.close() : help.showModal();
        break;
      case "r":
      case "R":
        replay("storm", "Storm replay", $("#storm-btn"));
        break;
      case "d":
      case "D":
        replay("stable", "Dry run", $("#dry-btn"));
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
  document.addEventListener("visibilitychange", () => {
    if (!document.hidden) refresh();
  });

  setInterval(() => {
    if (!document.hidden) refresh();
  }, 2000);
  refresh();
}

wire();
