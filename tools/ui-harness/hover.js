// Helpers every suite can use.
//
// `:hover` cannot be triggered by a script, so `__fhInit()` copies every `:hover` rule of
// the app's stylesheet to a `.fh` twin; add the class to an element and it looks hovered.
window.__fhInit = () => {
  const sheet = [...document.styleSheets].find((s) => s.href && s.href.includes("styles.css"));
  let n = 0;
  for (const rule of [...sheet.cssRules]) {
    if (rule.selectorText && rule.selectorText.includes(":hover")) {
      try {
        sheet.insertRule(rule.selectorText.replace(/:hover/g, ".fh") + "{" + rule.style.cssText + "}", sheet.cssRules.length);
        n++;
      } catch (e) {}
    }
  }
  return n;
};
window.__nav = (view) => document.querySelector('.nav[data-view="' + view + '"]').click();
window.__wait = (ms) => new Promise((r) => setTimeout(r, ms));
// The suite for one control, by its visible name inside the Settings tab.
window.__opt = (name) => [...document.querySelectorAll("#opts .opt")].find((r) => r.querySelector(".opt-name")?.textContent === name);
