// Run: node scripts/smoke_scene.mjs. No browser or packages needed.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";

const source = readFileSync(new URL("../web/app.js", import.meta.url), "utf8");
const html = readFileSync(new URL("../web/index.html", import.meta.url), "utf8");
const elements = new Map();
const element = (selector) => {
  if (!elements.has(selector)) elements.set(selector, {
    dataset: {}, style: {}, attributes: {}, textContent: "",
    classList: {
      _set: new Set(),
      toggle(name, on) { if (on) this._set.add(name); else this._set.delete(name); },
      contains(name) { return this._set.has(name); },
    },
    toggleAttribute(name, force) {
      const on = force ?? !(name in this.attributes);
      if (on) this.attributes[name] = ""; else delete this.attributes[name];
      if (name === "hidden") this.hidden = on;
      return on;
    },
    setAttribute(name, value) { this.attributes[name] = String(value); },
  });
  return elements.get(selector);
};
const context = vm.createContext({
  document: { querySelector: element, hidden: false },
  localStorage: { getItem: () => null },
  navigator: { language: "en" },
  matchMedia: () => ({ matches: true }),
});
vm.runInContext(source.replace(/\nwire\(\);\s*$/, "") + `
  globalThis.draw = (status, quality, relative, error = null, packetStatus = "OK", cursor = null) => {
    state.data = {
      rule: { state: status, quality, inputs: { rel_pitch_deg: relative } },
      series: { movement: { points: [[0, 0, 0, relative, null, 0, 70, null, null, 1, packetStatus]] } },
    };
    state.error = error;
    state.cursor = cursor;
    renderScene();
  };
`, context);

for (const [status, ground, relative, rotation] of [
  ["STABLE", "stable", 0.05, 0.2],
  ["WATCH", "watch", 0.65, 2.6],
  ["MOVEMENT_RISK", "risk", 2.8, 10.2],
  ["CRITICAL", "critical", 5.4, 18.6],
]) {
  context.draw(status, "GOOD", relative);
  assert.equal(element("#slope").dataset.ground, ground);
  assert.equal(element("#move-mast").style.transform, `rotate(${rotation.toFixed(2)}deg)`);
  assert.equal(element("#move-angle").textContent, `+${relative.toFixed(2)}°`);
}
for (const args of [
  ["CRITICAL", "DEGRADED", 5.4],
  ["CRITICAL", "GOOD", 5.4, "offline"],
  ["CRITICAL", "GOOD", 5.4, null, "FAULT"],
  ["UNKNOWN", "GOOD", 5.4],
  ["STABLE", "GOOD", null],
  ["CRITICAL", "GOOD", 5.4, null, "OK", 0],
]) {
  context.draw(...args);
  assert.equal(element("#slope").dataset.ground, "unknown");
}
context.draw("STABLE", "GOOD", null);
assert.equal(element("#move-angle").textContent, "—");
context.draw("MOVEMENT_RISK", "GOOD", -2.8);
assert.equal(element("#move-mast").style.transform, "rotate(-12.20deg)");
context.draw("STABLE", "GOOD", 99);
assert.equal(element("#move-mast").style.transform, "rotate(22.00deg)");
context.draw("STABLE", "GOOD", 0);
assert.equal(element("#angle-arc").attributes.d, "");
assert.equal(element(".scene").classList.contains("is-unverified"), false);
context.draw("UNKNOWN", "GOOD", 5.4);
assert.equal(element(".scene").classList.contains("is-unverified"), true);
context.draw("STABLE", "GOOD", 0);
assert.equal(element(".scene").classList.contains("is-unverified"), false);

for (const [hour, minute, sunVisible] of [
  [5, 59, false], [6, 0, true], [12, 0, true], [17, 59, true], [18, 0, false], [23, 30, false],
]) {
  context.renderSky(new Date(2026, 10, 28, hour, minute));
  assert.equal(element("#sun").hidden, !sunVisible, `sun at ${hour}:${minute}`);
  assert.equal(element("#moon").hidden, sunVisible, `moon at ${hour}:${minute}`);
  assert.equal(element("#sun").attributes.hidden !== undefined, !sunVisible, `sun attribute at ${hour}:${minute}`);
  assert.equal(element("#moon").attributes.hidden !== undefined, sunVisible, `moon attribute at ${hour}:${minute}`);
}

const svg = html.match(/<svg\b[^>]*id="slope"[\s\S]*?<\/svg>/)?.[0];
assert.ok(svg);
const ids = [...html.matchAll(/\bid="([^"]+)"/g)].map((match) => match[1]);
assert.equal(new Set(ids).size, ids.length, "duplicate document IDs");
assert.match(svg, /id="soil-moving"[\s\S]*id="mast-move"[\s\S]*<\/g>\s*<\/g>/);
assert.match(svg, /viewBox="230 140 670 460"/);
assert.doesNotMatch(svg, /mast-ref|ref-mast|bedrock|ridge|slope-rock|water|ghost|soil-layer|slip-line/);
assert.doesNotMatch(source, /\$\("#(?:mast-ref|ref-mast|water|water-label|ghost)"\)/);
assert.match(svg, /class="scenery" aria-hidden="true"/);
assert.match(svg, /decoration, not data/);
assert.match(svg, /id="mast-move"[^>]*translate\(610 385\)/);
assert.match(svg, /focusable="false"/);
assert.match(svg, /<g id="sun"[^>]*aria-hidden="true"/);
assert.match(svg, /<g id="moon"[^>]*aria-hidden="true"/);
assert.match(svg, /moon-cut/);
assert.match(svg, /class="svg-label sensor-question"[^>]*aria-hidden="true">\?<\/text>/);
assert.match(svg, /<g class="sensor-parts">[\s\S]*<\/g>\s*<text[^>]*class="svg-label node-tag"/);
console.log("scene smoke: states, trust guards, signed tilt, clamp, framing, single sensor, removed decoration, IDs passed");
