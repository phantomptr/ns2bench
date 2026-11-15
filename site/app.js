// NS2Bench website: the language, the light/dark switch, copy buttons, platform-aware download
// links and the crypto QR codes. No dependencies. Used by the home page and by the FAQ and setup
// pages that scripts/site-build.mjs renders from Markdown.
//
// The words come in the visitor's language: English is in the HTML, the others in
// i18n/<code>.json (scripts/site-i18n.mjs --check keeps every file complete). The language itself
// is picked in each page's <head>, before anything is drawn: ?lang=, then the saved choice, then
// the browser's languages. The FAQ and setup pages carry every language's text; the visitor's is
// swapped in from its <template data-lang>.
"use strict";

(() => {
  const root = document.documentElement;
  const base = new URL("./", document.currentScript?.src || location.href);
  const store = {
    get: (k) => {
      try {
        return localStorage.getItem(k);
      } catch {
        return null;
      }
    },
    set: (k, v) => {
      try {
        localStorage.setItem(k, v);
      } catch {
        /* private window: the choice lasts this visit */
      }
    },
  };

  // ── Language ─────────────────────────────────────────────────────────────

  /** The app's languages, in the app's order. */
  const LANGS = [
    ["en", "English"],
    ["zh-CN", "简体中文"],
    ["zh-TW", "繁體中文"],
    ["ja", "日本語"],
    ["ko", "한국어"],
    ["es", "Español"],
    ["pt-BR", "Português (Brasil)"],
    ["fr", "Français"],
    ["de", "Deutsch"],
    ["it", "Italiano"],
    ["ru", "Русский"],
  ];
  const lang = LANGS.some(([c]) => c === root.lang) ? root.lang : "en";
  let words = {};

  /** A string in the visitor's language, `en` when it has no translation; {name} takes vars.name. */
  function t(key, en, vars = {}) {
    let s = words[key] ?? en;
    for (const [k, v] of Object.entries(vars)) s = s.replaceAll(`{${k}}`, v);
    return s;
  }

  async function loadWords() {
    if (lang === "en") return;
    try {
      const res = await fetch(new URL(`i18n/${lang}.json`, base));
      if (res.ok) words = await res.json();
    } catch {
      /* stays English */
    }
  }

  /** data-i18n is an element's content (which may hold <b> and <a>), data-i18n-attr is
   *  "attribute:key" pairs split by ";". */
  function translatePage() {
    document.querySelectorAll("[data-i18n]").forEach((el) => {
      const s = words[el.dataset.i18n];
      if (s !== undefined) el.innerHTML = s;
    });
    document.querySelectorAll("[data-i18n-attr]").forEach((el) => {
      for (const pair of el.dataset.i18nAttr.split(";")) {
        const [attr, key] = pair.split(":");
        if (words[key] !== undefined) el.setAttribute(attr, words[key]);
      }
    });
  }

  /** The FAQ and setup pages: show the article in the visitor's language. */
  function showArticle() {
    const current = document.querySelector("[data-doc]");
    if (!current || lang === "en") return false;
    const tpl = document.querySelector(`template[data-lang="${lang}"]`);
    if (!tpl) {
      const note = document.querySelector("[data-doc-fallback]");
      if (note) note.hidden = false;
      return false;
    }
    current.replaceWith(tpl.content.cloneNode(true));
    return true;
  }

  const select = document.getElementById("lang");
  if (select) {
    select.innerHTML = LANGS.map(([c, label]) => `<option value="${c}" lang="${c}">${label}</option>`).join("");
    select.value = lang;
    select.addEventListener("change", () => {
      store.set("ns2bench.site.lang", select.value);
      const url = new URL(location.href);
      url.searchParams.set("lang", select.value);
      location.href = url.href;
    });
  }

  // ── Theme ────────────────────────────────────────────────────────────────

  const systemDark = () => window.matchMedia("(prefers-color-scheme: dark)").matches;
  document.getElementById("theme")?.addEventListener("click", () => {
    const dark = root.dataset.theme ? root.dataset.theme === "dark" : systemDark();
    const next = dark ? "light" : "dark";
    store.set("ns2bench.site.theme", next);
    root.dataset.theme = next;
  });

  // ── Copy buttons ─────────────────────────────────────────────────────────

  async function copy(btn, text) {
    try {
      await navigator.clipboard.writeText(text);
      btn.textContent = t("common.copied", "Copied");
      setTimeout(() => (btn.textContent = t("common.copy", "Copy")), 1500);
    } catch {
      /* no clipboard access: the text stays selectable */
    }
  }
  document.addEventListener("click", (e) => {
    const btn = e.target.closest("[data-copy]");
    if (btn) copy(btn, document.getElementById(btn.dataset.copy)?.textContent ?? "");
  });

  // ── Downloads (asset names follow publish.yml) ───────────────────────────

  function setupDownloads() {
    const version = document.querySelector('meta[name="ns2bench-version"]')?.content;
    if (!version) return;
    const release = `https://github.com/phantomptr/ns2bench/releases/download/v${version}/`;
    for (const a of document.querySelectorAll("[data-asset]")) a.href = `${release}NS2Bench-${version}-${a.dataset.asset}`;
    for (const a of document.querySelectorAll("[data-file]")) a.href = release + a.dataset.file;
    const ua = navigator.userAgent;
    const os = /Windows/.test(ua) ? "win" : /Mac/.test(ua) && !/iPhone|iPad/.test(ua) ? "mac" : /Linux/.test(ua) && !/Android/.test(ua) ? "linux" : null;
    if (!os) return;
    const card = document.querySelector(`.dl[data-os="${os}"]`);
    if (card) {
      card.classList.add("mine");
      card.querySelector(".tag")?.removeAttribute("hidden");
    }
    const asset = { win: "win-x64-setup.exe", mac: "mac-arm64.dmg", linux: "linux-x64.AppImage" }[os];
    const main = document.getElementById("dl-main");
    const label = document.getElementById("dl-main-label");
    if (main) main.href = `${release}NS2Bench-${version}-${asset}`;
    if (label) {
      label.textContent = {
        win: t("hero.download_win", "Download for Windows"),
        mac: t("hero.download_mac", "Download for Mac"),
        linux: t("hero.download_linux", "Download for Linux"),
      }[os];
    }
  }

  // ── Thanks & Support ─────────────────────────────────────────────────────

  // Public receiving addresses (the owner's hardware wallet). QR codes are assets/donate/<id>.svg.
  const CRYPTO = [
    { id: "btc", coin: "BTC", name: "Bitcoin", network: "Bitcoin", color: "#f7931a", address: "bc1q36gxkvd3wfxy9azm8hft3rru9m6m9hjvn7u20n" },
    { id: "eth", coin: "ETH", name: "Ethereum", network: "Ethereum", color: "#627eea", address: "0x2e787654db543B69FB011E16995C274dFAfc2286" },
    { id: "sol", coin: "SOL", name: "Solana", network: "Solana", color: "#9945ff", address: "G1q5XCCn3mePYpcx7uY17CwDsUaAZTcty56huUDvVqjG" },
    { id: "xrp", coin: "XRP", name: "XRP", network: "XRP Ledger", color: "#2a9df4", address: "rEVq3si3sm9NuWNuZiUZFKsUtwC3w9SrjR" },
  ];

  function showCoin(id) {
    const c = CRYPTO.find((x) => x.id === id) ?? CRYPTO[0];
    document.querySelectorAll("#coin-tabs button").forEach((b) => {
      const on = b.dataset.coin === c.id;
      b.setAttribute("aria-selected", String(on));
      b.tabIndex = on ? 0 : -1;
      b.style.background = on ? c.color : "";
      b.style.color = on ? "#fff" : "";
    });
    document.getElementById("coin-panel").setAttribute("aria-labelledby", `coin-tab-${c.id}`);
    const qr = document.getElementById("coin-qr");
    qr.src = new URL(`assets/donate/${c.id}.svg`, base).href;
    qr.alt = t("thanks.qr", "{coin} QR code", { coin: c.name });
    qr.style.borderColor = c.color;
    const name = document.getElementById("coin-name");
    name.textContent = c.name;
    name.style.color = c.color;
    document.getElementById("coin-network").textContent = t("thanks.network", "Network: {network}", { network: c.network });
    document.getElementById("coin-address").textContent = c.address;
    document.getElementById("coin-warn").textContent = t(
      "thanks.network_only",
      "Send only {coin} on the {network} network. Anything else can't be recovered.",
      { coin: c.coin, network: c.network },
    );
    document.getElementById("coin-note").hidden = c.id !== "xrp";
  }

  function setupThanks() {
    const tabs = document.getElementById("coin-tabs");
    if (!tabs) return;
    tabs.innerHTML = CRYPTO.map(
      (c) => `<button type="button" role="tab" id="coin-tab-${c.id}" aria-controls="coin-panel" data-coin="${c.id}">${c.coin}</button>`,
    ).join("");
    tabs.addEventListener("click", (e) => {
      const b = e.target.closest("button[data-coin]");
      if (b) showCoin(b.dataset.coin);
    });
    // A tab list is one stop for Tab; the arrows move inside it.
    tabs.addEventListener("keydown", (e) => {
      const all = [...tabs.querySelectorAll("button[data-coin]")];
      const at = all.indexOf(document.activeElement);
      if (at < 0) return;
      let next = null;
      if (e.key === "ArrowRight") next = all[(at + 1) % all.length];
      else if (e.key === "ArrowLeft") next = all[(at - 1 + all.length) % all.length];
      else if (e.key === "Home") next = all[0];
      else if (e.key === "End") next = all[all.length - 1];
      if (!next) return;
      e.preventDefault();
      showCoin(next.dataset.coin);
      next.focus();
    });
    showCoin("btc");
    const btn = document.getElementById("coin-copy");
    btn.addEventListener("click", () => copy(btn, document.getElementById("coin-address").textContent));
  }

  loadWords().then(() => {
    showArticle();
    translatePage();
    setupDownloads();
    setupThanks();
    root.classList.remove("i18n-wait");
    // The page's text changed under a #link: go to where it points now.
    if (lang !== "en" && location.hash.length > 1) {
      document.getElementById(decodeURIComponent(location.hash.slice(1)))?.scrollIntoView();
    }
  });
})();
