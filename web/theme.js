try {
  var saved = localStorage.getItem("sg-theme");
  var light = matchMedia("(prefers-color-scheme: light)").matches;
  var theme = saved === "light" || saved === "dark" ? saved : light ? "light" : "dark";
  document.documentElement.dataset.theme = theme;
  var meta = document.querySelector('meta[name="color-scheme"]');
  if (meta) meta.content = theme;
} catch (e) {}
