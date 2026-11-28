// Run: node scripts/smoke_weather.mjs
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";

const source = readFileSync(new URL("../web/app.js", import.meta.url), "utf8");
const html = readFileSync(new URL("../web/index.html", import.meta.url), "utf8");
const elements = new Map();
const node = () => ({ hidden: false, textContent: "", children: [], replaceChildren(...items) { this.children = items; } });
const get = (selector) => {
  if (!elements.has(selector)) elements.set(selector, node());
  return elements.get(selector);
};
const context = vm.createContext({
  document: { querySelector: get, createElement: node },
  localStorage: { getItem: () => null },
  navigator: { language: "en" },
  matchMedia: () => ({ matches: true }),
});
vm.runInContext(source.replace(/\nwire\(\);\s*$/, "") + `
  globalThis.view = (weather, language = "en", error = null) => {
    lang = language;
    state.data = { weather, rule: { state: "UNKNOWN" } };
    state.error = error;
    renderWeather();
    return state.data.rule.state;
  };
`, context);

assert.match(html, /id="weather"/);
assert.match(html, /id="site-form"/);
assert.match(html, /id="site-adm4"/);
assert.match(html, /https:\/\/data\.bmkg\.go\.id\/prakiraan-cuaca\//);
const data = {
  status: "ready", source: "BMKG", adm4: "31.71.03.1001", area: "Kemayoran", city: "Jakarta Pusat",
  timezone: "Asia/Jakarta", fetched_at: "2026-11-28T03:00:00Z",
  slots: [{ valid_at: "2026-11-28T06:00:00Z", issued_at: "2026-11-28T00:00:00Z", condition_en: "Light Rain", condition_id: "Hujan Ringan", temperature_c: 28, humidity_pct: null }],
};
assert.equal(context.view(data), "UNKNOWN");
assert.match(get("#weather-status").textContent, /Kemayoran/);
assert.equal(get("#weather-rows").children[0].children[1].textContent, "Light Rain");
assert.match(get("#weather-rows").children[0].children[0].children[0].textContent, /13:00/);
assert.equal(get("#weather-rows").children[0].children[3].textContent, "—");
context.view({ ...data, status: "stale" }, "id");
assert.match(get("#weather-status").textContent, /prakiraan tersimpan/i);
assert.equal(get("#weather-rows").children[0].children[1].textContent, "Hujan Ringan");
context.view(data, "en", "offline");
assert.equal(get("#weather-table").hidden, true);
assert.match(get("#weather-status").textContent, /unavailable/i);
context.view({ status: "disabled" });
assert.match(get("#weather-status").textContent, /ward code/);
console.log("Weather panel: source, local time, localization, nulls, outage, disabled, unchanged verdict passed");
