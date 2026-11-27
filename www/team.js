// cairn-plan team page (served at /team). Each configured member is drawn as a
// pebble character; its state comes from the runs its agents report
// (GET /api/runs), its queue from the open tickets assigned to it (GET /api/team).
// The page reads again every 10 seconds, only while the tab is visible. "Replay" plays the last 24 hours fast.
"use strict";
// the key of a run is the pair (account, run_id): two accounts may send the same run_id
const rid = r => (r.login || "") + "\0" + r.run_id;

const $ = id => document.getElementById(id);
const esc = s => String(s == null ? "" : s).replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const nowS = () => Date.now() / 1000;
const STALE = 6 * 3600;       // a run "running" without news for this long is not shown as working
const WINDOW = 7 * 86400;     // runs read for the page
const REFRESH = 10000;

let ME = null, TEAM = { groups: [], members: [] }, RUNS = [], RATES = {}, M = [];
let mode = "live", clock = 0, openId = null, seen = new Set();

/* ---------- formatting ---------- */
const fT = () => new Intl.DateTimeFormat(LOCALE(), { hour: "2-digit", minute: "2-digit" });
const fD = () => new Intl.DateTimeFormat(LOCALE(), { day: "numeric", month: "short" });
const hm = s => fT().format(new Date(s * 1000));
const when = s => (new Date(s * 1000).toDateString() === new Date().toDateString() ? "" : fD().format(new Date(s * 1000)) + " ") + hm(s);
const ago = s => s < 60 ? t("{n} s", { n: Math.max(1, Math.round(s)) }) : s < 3600 ? t("{n} min", { n: Math.round(s / 60) })
  : s < 86400 ? t("{n} h", { n: Math.round(s / 3600) }) : t("{n} d", { n: Math.round(s / 86400) });
const model = m => { const x = /^claude-([a-z]+)-(\d+)-(\d+)/.exec(m || ""); return x ? x[1][0].toUpperCase() + x[1].slice(1) + " " + x[2] + "." + x[3] : (m || ""); };
const fmt = (n, code) => new Intl.NumberFormat(LOCALE(), { style: "currency", currency: code, minimumFractionDigits: Math.abs(n) < 100 ? 2 : 0, maximumFractionDigits: Math.abs(n) < 100 ? 2 : 0 }).format(n);
const money = usd => ME && ME.money ? fmt(usd * (ME.money.fx[ME.money.currency] || 1), ME.money.currency) : usd.toFixed(2) + " $";
const HUE = h => /^#[0-9a-f]{6}$/i.test(h || "") ? h : "#8c7d6b";

/* ---------- data ---------- */
async function get(path) {
  const r = await fetch(path, { redirect: "manual" });
  if (r.type === "opaqueredirect" || r.status === 401) { location.reload(); throw new Error("signed out"); }
  if (!r.ok) throw new Error(path + " " + r.status);
  return r.json();
}

const verdict = r => {
  if (r.state === "failed") return ["bad", r.result || t("Failed")];
  if (r.state === "waiting") return ["warn", r.result || t("Waiting for an answer")];
  if (!r.result) return ["neutral", t("Done")];
  return [/écart|ecart|gap|fail|échec|refus|non conforme|not compliant/i.test(r.result) ? "warn" : "ok", r.result];
};

/** Whether an agent name is one of the member's: exact, or a prefix written "name*". */
const agentOf = m => agent => (m.agents || []).some(a => a.endsWith("*") ? agent.startsWith(a.slice(0, -1)) : agent === a);

const OTHERS = "others";   // implicit member: the runs of the agents that no member takes

/** Members with their runs, queue and state, from the data read last. */
function build() {
  const members = TEAM.members.slice();
  // only when a team is set and some run belongs to nobody (a member whose id is "others" is left as it is)
  if (members.length && !members.some(m => m.id === OTHERS) && RUNS.some(r => !members.some(m => agentOf(m)(r.agent))))
    members.push({ id: OTHERS, name: t("Others"), role: t("Agents that no member takes"), group: OTHERS, look: "", hue: "#8c7d6b", others: true });
  M = members.map(m => {
    const mine = m.others ? agent => !TEAM.members.some(x => agentOf(x)(agent)) : agentOf(m);
    const runs = RUNS.filter(r => mine(r.agent)).sort((a, b) => b.started - a.started);
    const rated = Object.keys(RATES).filter(mine);
    const usd = rated.reduce((s, a) => s + RATES[a].usd, 0);
    const hours = rated.reduce((s, a) => s + RATES[a].hours, 0);
    return {
      ...m, hue: HUE(m.hue), runs,
      cur: runs.filter(r => r.state === "running" && nowS() - r.updated < STALE).map(r => ({ k: rid(r), task: r.task, ticket: r.ticket, since: r.started })),
      stale: runs.filter(r => r.state === "running" && nowS() - r.updated >= STALE),
      done: runs.filter(r => r.state !== "running").map(item),
      queue: m.queue || [],
      waitRun: runs[0] && runs[0].state === "waiting" ? runs[0] : null,
      rate: hours >= 1 / 60 ? usd / hours : null,
    };
  });
}
function item(r) {
  const end = r.ended || r.updated;
  const cost = r.cost_counted > 0 ? (r.cost_estimated ? "≈ " : "") + money(r.cost_counted) : "";
  const dur = r.ended && r.ended - r.started < 43200 ? ago(r.ended - r.started) : "";
  return { key: rid(r), t: end, title: r.task, ticket: r.ticket, meta: [model(r.model), dur, cost].filter(Boolean).join(" · "), v: verdict(r) };
}

/* ---------- state ---------- */
const now = () => mode === "replay" ? clock : nowS();
const stateOf = m => m.cur.length ? "work" : m.waitRun || m.queue.some(q => q.state === "waiting") ? "wait" : "rest";
function statusLine(m) {
  const s = stateOf(m);
  if (s === "work") return t("Working · for {t}", { t: ago(now() - Math.min(...m.cur.map(c => c.since))) });
  if (s === "wait") return t("Waiting for an answer");
  if (m.stale.length) return t("No news for {t}", { t: ago(now() - m.stale[0].updated) });
  return t("Resting");
}
function taskLine(m) {
  const s = stateOf(m);
  if (s === "work") return m.cur[m.cur.length - 1].task;
  if (s === "wait") { const q = m.waitRun ? { id: m.waitRun.ticket, title: m.waitRun.task } : m.queue.find(x => x.state === "waiting"); return (q.id ? q.id + " : " : "") + q.title; }
  const last = m.done[0];
  if (m.rest) return m.rest;
  return last ? t("Last run {t} ago", { t: ago(now() - last.t) }) : t("No run yet.");
}

/* ---------- portraits ---------- */
const HEAD = "M34 58C33 40 46 33 61 33C77 33 88 42 87 57C86 72 75 80 60 80C45 80 35 73 34 58Z";
const BODY = "M13 124C13 97 30 83 60 83C90 83 107 97 107 124Z";
const EARS = '<path class="dk" d="M37 45C24 46 21 70 29 80C36 74 39 61 41 52Z"/><path class="dk" d="M83 45C96 46 99 70 91 80C84 74 81 61 79 52Z"/>';
const TAG = '<circle cx="60" cy="99" r="5.5" fill="#f2c94c" stroke="#a37b12" stroke-width="1.2"/>';
// Accessory of each look: [behind the figure, in front of it]. Unknown look: a plain pebble.
const LOOKS = {
  conductor: ["", `<path class="dks" d="M45 38C52 29 70 29 77 37" stroke-width="5" stroke-linecap="round"/>
    <path class="ink" d="M53 87L44 82V92Z M67 87L76 82V92Z"/><circle class="ink" cx="60" cy="87" r="3.2"/>
    <ellipse class="hd" cx="89" cy="101" rx="7" ry="6"/><path d="M92 97L108 62" stroke="#2a2a33" stroke-width="3" stroke-linecap="round"/><circle cx="108" cy="62" r="2.6" fill="#ffffff"/>`],
  hardhat: ["", `<path d="M33 51C33 28 87 28 87 51Z" fill="#f2b51d"/><path d="M60 29V50" stroke="#d99a0c" stroke-width="5"/>
    <rect x="27" y="48" width="66" height="7" rx="3.5" fill="#e0a10f"/>
    <path d="M84 116L99 95" stroke="#7b8494" stroke-width="5.5" stroke-linecap="round"/><circle cx="101" cy="92" r="6.5" fill="none" stroke="#7b8494" stroke-width="4"/>`],
  glasses: ["", `<circle cx="51" cy="58" r="8" fill="#ffffff" fill-opacity=".28" stroke="#22232b" stroke-width="2.4"/>
    <circle cx="69" cy="58" r="8" fill="#ffffff" fill-opacity=".28" stroke="#22232b" stroke-width="2.4"/><path class="inks" d="M59 57.5Q60 56 61 57.5" stroke-width="2.2"/>
    <path d="M100 112L108 124" stroke="#3a3b44" stroke-width="5" stroke-linecap="round"/><circle cx="93" cy="99" r="12" fill="#ffffff" fill-opacity=".35" stroke="#22232b" stroke-width="3"/>`],
  nightcap: [`<path d="M27 20A11 11 0 1 0 38 35A9 9 0 1 1 27 20Z" fill="#f4e7b5"/><circle cx="44" cy="17" r="1.4" fill="#f4e7b5"/><circle cx="18" cy="42" r="1.1" fill="#f4e7b5"/>`,
    `<path d="M34 51C35 30 63 24 82 37C93 45 100 58 101 71C93 62 86 56 85 51Z" fill="#efe6cf"/><path d="M34 51C40 47 78 45 86 51" stroke="#d9ccab" stroke-width="5" stroke-linecap="round" fill="none"/><circle cx="101" cy="72" r="6" fill="#ffffff"/>`],
  "dog-headset": ["", EARS + TAG + `<path d="M34 57C34 25 86 25 86 57" fill="none" stroke="#2a2a33" stroke-width="4"/><rect x="28" y="51" width="9" height="15" rx="3.5" fill="#2a2a33"/><rect x="83" y="51" width="9" height="15" rx="3.5" fill="#2a2a33"/><path d="M33 65Q36 75 47 74" fill="none" stroke="#2a2a33" stroke-width="2.5" stroke-linecap="round"/><circle cx="48" cy="74" r="2.6" fill="#2a2a33"/>`],
  "dog-laptop": ["", EARS + `<rect x="33" y="89" width="54" height="31" rx="4" fill="#2a2d38"/><path d="M53 99L47 104L53 109M67 99L73 104L67 109" fill="none" stroke="#9ef0d8" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"/>`],
  "dog-book": ["", EARS + TAG + `<path d="M73 33L95 47" stroke="#f2c94c" stroke-width="5.5" stroke-linecap="round"/><path d="M95 47L99 50" stroke="#3a3b44" stroke-width="3" stroke-linecap="round"/>
    <path d="M28 103Q44 96 60 103Q76 96 92 103V121Q76 114 60 121Q44 114 28 121Z" fill="#fffdf6" stroke="#cfc6b0" stroke-width="1.5"/><path d="M60 103V121" stroke="#cfc6b0" stroke-width="1.5"/><path d="M36 108H52M36 113H50M68 108H84M68 113H82" stroke="#cfc6b0" stroke-width="1.4"/>`],
  cairn: ["", `<ellipse cx="60" cy="31" rx="12" ry="5" fill="#a69e90"/><ellipse cx="60" cy="23" rx="9" ry="4" fill="#bdb5a8"/><ellipse cx="60" cy="16.5" rx="6" ry="3" fill="#d3ccc0"/>
    <g transform="rotate(8 90 104)"><rect x="80" y="91" width="21" height="27" rx="2.5" fill="#ffffff" stroke="#cfc8bb" stroke-width="1.4"/><path d="M85 99H96M85 104H96M85 109H92" stroke="#b5ad9f" stroke-width="1.6"/></g>`],
  courier: ["", `<path d="M35 47C37 29 83 29 85 47Z" fill="#2f3b52"/><path d="M58 45C72 42 92 44 97 51C84 51 70 51 58 49Z" fill="#1f2838"/>
    <rect x="68" y="92" width="34" height="27" rx="2" fill="#c99a62"/><path d="M68 101H102" stroke="#a87a43" stroke-width="3"/><path d="M85 113V104M81 108L85 104L89 108" fill="none" stroke="#ffffff" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/>`],
  toolbox: ["", `<path d="M36 46C38 30 82 30 84 46Z" fill="#3b6e8f"/><rect x="34" y="44" width="52" height="5" rx="2.5" fill="#2f5a75"/>
    <rect x="58" y="90" width="38" height="21" rx="3" fill="#d6453a"/><path d="M69 90V85H85V90" fill="none" stroke="#8f2a22" stroke-width="3"/><path d="M58 98H96" stroke="#8f2a22" stroke-width="2"/>`],
  binoculars: ["", `<path d="M38 44C40 31 80 31 82 44Z" fill="#6b8f3b"/><path d="M38 44H90" stroke="#55732f" stroke-width="4" stroke-linecap="round"/>
    <rect x="62" y="86" width="13" height="19" rx="5" fill="#2a2d38"/><rect x="79" y="86" width="13" height="19" rx="5" fill="#2a2d38"/><rect x="73" y="92" width="8" height="6" fill="#2a2d38"/>
    <circle cx="68.5" cy="103" r="4.5" fill="#9fd1ff"/><circle cx="85.5" cy="103" r="4.5" fill="#9fd1ff"/>`],
  shield: ["", `<g transform="translate(-12 -3)"><path d="M70 86L90 90V103C90 111 82 117 80 118C78 117 70 111 70 103Z" fill="#e8edf5" stroke="#2f3e52" stroke-width="2.4" stroke-linejoin="round"/>
    <rect x="75.5" y="99" width="9" height="8" rx="1.5" fill="#2f3e52"/><path d="M77.5 99V96.5A2.5 2.5 0 0 1 82.5 96.5V99" fill="none" stroke="#2f3e52" stroke-width="1.8"/></g>`],
  scales: ["", `<g transform="translate(-16 -4)"><path d="M88 84V114M80 114H96" stroke="#9a7a1c" stroke-width="3" stroke-linecap="round"/><path d="M72 90H104" stroke="#c9a227" stroke-width="3" stroke-linecap="round"/>
    <path d="M72 90L67 101M72 90L77 101M104 90L99 101M104 90L109 101" stroke="#9a7a1c" stroke-width="1.2"/>
    <path d="M65 101H79A7 4 0 0 1 65 101Z" fill="#c9a227"/><path d="M97 101H111A7 4 0 0 1 97 101Z" fill="#c9a227"/></g>`],
  tie: ["", `<path d="M49 85L60 93L71 85" fill="none" stroke="#ffffff" stroke-width="3.2" stroke-linecap="round" stroke-linejoin="round"/>
    <path d="M56.5 92H63.5L62 97L65.5 112L60 118L54.5 112L58 97Z" fill="#b23a48"/>
    <g transform="translate(-11 -9)"><rect x="76" y="99" width="27" height="19" rx="3" fill="#5b4636"/><path d="M84 99V94.5H95V99" fill="none" stroke="#3d2f24" stroke-width="2.6"/><path d="M76 107H103" stroke="#3d2f24" stroke-width="1.6"/></g>`],
  coins: ["", `<path d="M33 49C40 41 80 41 87 49L93 53C78 48 42 48 27 53Z" fill="#2f8f6b"/>
    <g transform="translate(-9 -9)"><ellipse cx="88" cy="113" rx="13" ry="4.5" fill="#b8901c"/><ellipse cx="88" cy="108" rx="13" ry="4.5" fill="#c9a227"/><ellipse cx="88" cy="103" rx="13" ry="4.5" fill="#dcb93a"/><ellipse cx="88" cy="98" rx="13" ry="4.5" fill="#efcf58"/>
    <path d="M83 98H93" stroke="#9a7a1c" stroke-width="1.6" stroke-linecap="round"/></g>`],
  chart: ["", `<rect x="64" y="88" width="38" height="28" rx="3" fill="#ffffff" stroke="#b9c2cf" stroke-width="1.5"/>
    <path d="M70 110L78 101L85 105L96 94" fill="none" stroke="#2e7d6b" stroke-width="2.8" stroke-linecap="round" stroke-linejoin="round"/><circle cx="96" cy="94" r="2.6" fill="#2e7d6b"/>`],
  scroll: ["", `<g transform="rotate(-9 86 103)"><rect x="72" y="88" width="27" height="31" rx="2" fill="#fbf4df" stroke="#cdbf96" stroke-width="1.4"/>
    <path d="M77 95H94M77 100H94M77 105H88" stroke="#bfb08a" stroke-width="1.6" stroke-linecap="round"/><circle cx="91" cy="112" r="4.2" fill="#b23a48"/></g>`],
  horns: ["", `<path d="M44 40C38 33 38 24 43 19C45 27 49 32 54 35Z" fill="#b23a48"/><path d="M76 40C82 33 82 24 77 19C75 27 71 32 66 35Z" fill="#b23a48"/>
    <g transform="translate(-9 -6)"><circle cx="90" cy="102" r="12" fill="#ffffff" stroke="#b23a48" stroke-width="2.4"/>
    <path d="M86 98.5C86 93 94 93 94 98.5C94 101.5 90 101.5 90 105" fill="none" stroke="#b23a48" stroke-width="2.6" stroke-linecap="round"/><circle cx="90" cy="109.2" r="1.6" fill="#b23a48"/></g>`],
  gavel: ["", `<g transform="translate(-10 -9)"><g transform="rotate(-28 88 100)"><rect x="75" y="88" width="26" height="12" rx="3.5" fill="#7a5230"/><rect x="86" y="99" width="4.5" height="22" rx="2.2" fill="#a9773f"/></g>
    <rect x="66" y="115" width="26" height="4.5" rx="2.2" fill="#5b3d22"/></g>`],
  quill: ["", `<g transform="translate(-5 -7)"><rect x="61" y="106" width="15" height="11" rx="2.5" fill="#2a2d38"/><rect x="64" y="103" width="9" height="4" rx="1.5" fill="#3a3d4a"/>
    <path d="M100 78C91 83 82 95 78 110L82 111C90 102 98 92 100 78Z" fill="#f4f1e8" stroke="#b9b3a2" stroke-width="1.2" stroke-linejoin="round"/><path d="M79 109L75 118" stroke="#3a3b44" stroke-width="2" stroke-linecap="round"/></g>`],
  beacon: [`<g class="beam"><path d="M60 8L50 -6H70Z" fill="#ef4444" fill-opacity=".35"/><path d="M44 16L28 8L34 22Z" fill="#ef4444" fill-opacity=".3"/><path d="M76 16L92 8L86 22Z" fill="#ef4444" fill-opacity=".3"/></g>`,
    `<path d="M33 52C33 27 87 27 87 52Z" fill="#e9eef5"/><rect x="29" y="49" width="62" height="6" rx="3" fill="#cfd7e2"/><path d="M52 30C52 19 68 19 68 30Z" fill="#ef4444"/>
    <rect x="44" y="92" width="32" height="20" rx="4" fill="#f4f6f9"/><path d="M50 97H70M50 102H64" stroke="#c4242a" stroke-width="2.2" stroke-linecap="round"/>`],
};
let pid = 0;
function portrait(m) {
  const [back, front] = LOOKS[m.look] || ["", ""], cid = "cp" + (++pid);
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("class", "pt"); svg.setAttribute("viewBox", "0 0 120 120"); svg.setAttribute("aria-hidden", "true");
  svg.innerHTML = `<defs><clipPath id="${cid}"><circle cx="60" cy="60" r="55"/></clipPath></defs>
    <circle class="bgc" cx="60" cy="60" r="55"/>
    <g clip-path="url(#${cid})">${back}<g class="fig">
      <path class="bd" d="${BODY}"/><path class="hd" d="${HEAD}"/>
      <ellipse cx="50" cy="45" rx="9" ry="4.5" fill="#ffffff" opacity=".35"/>
      <g class="eo"><circle class="ink" cx="51" cy="58" r="3.2"/><circle class="ink" cx="69" cy="58" r="3.2"/></g>
      <g class="ec"><path class="inks" d="M47 59Q51 62 55 59M65 59Q69 62 73 59" stroke-width="2.2"/></g>
      <path class="inks" d="M55 68Q60 72 65 68" stroke-width="2.2"/>${front}</g></g>
    <circle class="ring-base" cx="60" cy="60" r="57"/><circle class="ring" cx="60" cy="60" r="57"/>
    <g class="bub"><circle class="b" cx="99" cy="22" r="11" stroke-width="1.5"/><circle class="d" cx="94.5" cy="22" r="1.6"/><circle class="d" cx="99" cy="22" r="1.6"/><circle class="d" cx="103.5" cy="22" r="1.6"/></g>
    <g class="zz"><text x="92" y="30">z</text><text x="100" y="20" font-size="10">z</text></g>`;
  svg.style.setProperty("--h", m.hue);  // CSSOM: allowed by the CSP, unlike a style attribute
  return svg;
}

/* ---------- page ---------- */
function buildCards() {
  const box = $("teams"); box.textContent = "";
  if (!M.length) {
    box.innerHTML = `<p class="empty-page">${esc(t("No team is configured. An administrator lists the members in the \"team\" key of the configuration file."))}</p>`;
    return;
  }
  const groups = (TEAM.groups.length ? TEAM.groups : [{ id: "", title: "" }]).slice();
  if (M.some(m => m.others) && !groups.some(g => g.id === OTHERS)) groups.push({ id: OTHERS, title: "" });
  for (const g of groups) {
    const members = M.filter(m => (m.group || "") === (g.id || "") || (!groups.some(x => x.id === m.group) && g === groups[0]));
    if (!members.length) continue;
    const sec = document.createElement("section"); sec.className = "team";
    if (g.title) sec.innerHTML = `<h2>${esc(g.title)}${g.note ? ` <small>${esc(g.note)}</small>` : ""}</h2>`;
    const grid = document.createElement("div"); grid.className = "grid"; sec.appendChild(grid);
    for (const m of members) {
      const b = document.createElement("button");
      b.type = "button"; b.className = "agent"; b.id = "card-" + m.id;
      b.appendChild(portrait(m));
      b.insertAdjacentHTML("beforeend", `<span class="nm">${esc(m.name || m.id)}<span class="n"></span></span><span class="rl">${esc(m.role || "")}</span><span class="st"></span><span class="tk"></span>`);
      b.addEventListener("click", () => openFiche(m.id));
      grid.appendChild(b);
    }
    box.appendChild(sec);
  }
  M.forEach(updateCard);
}
function updateCard(m) {
  const b = $("card-" + m.id); if (!b) return;
  b.className = "agent s-" + stateOf(m);
  b.querySelector(".n").textContent = m.cur.length > 1 ? "×" + m.cur.length : "";
  b.querySelector(".st").textContent = statusLine(m);
  b.querySelector(".tk").textContent = taskLine(m);
  b.setAttribute("aria-label", `${m.name || m.id}. ${statusLine(m)}. ${taskLine(m)}. ${t("Open the sheet")}`);
}
function summary() {
  const c = { work: 0, wait: 0, rest: 0 };
  M.forEach(m => c[stateOf(m)]++);
  $("summary").innerHTML = esc(tp(M.length, "{n} member works for you.", "{n} members work for you.")) + " "
    + t("<b>{a}</b> working, <b>{b}</b> waiting, <b>{c}</b> resting.", { a: c.work, b: c.wait, c: c.rest }) + " "
    + esc(t("Open a member to see what it did and what it will do."));
  today();
}
/** One line under the title: what the members ran since midnight, and what it cost. */
function today() {
  const start = new Date(now() * 1000); start.setHours(0, 0, 0, 0);
  const since = start.getTime() / 1000;
  // a run that two members share (prefix names) counts once
  const runs = [...new Map(M.flatMap(m => m.runs).map(r => [r.login + "/" + r.run_id, r])).values()]
    .filter(r => r.started >= since && (mode !== "replay" || r.started <= clock));
  const usd = runs.reduce((s, r) => s + (r.cost_counted > 0 ? r.cost_counted : 0), 0);
  const est = runs.some(r => r.cost_estimated);
  $("today").textContent = runs.length
    ? tp(runs.length, "Today: {n} run", "Today: {n} runs") + (usd > 0 ? " · " + (est ? "≈ " : "") + money(usd) : "")
    : t("Today: no run yet.");
}

function chip() {
  $("chiptext").textContent = mode === "replay" ? t("Replay · {t}", { t: when(clock) })
    : document.body.classList.contains("offline") ? t("Offline") : t("Live · {t}", { t: hm(nowS()) });
}
function tick() {
  M.forEach(m => { const b = $("card-" + m.id); if (b) b.querySelector(".st").textContent = statusLine(m); });
  if (openId) { const st = document.querySelector("#ficheIn .now .st"); const m = M.find(x => x.id === openId); if (st && m) st.textContent = statusLine(m); }
  chip();
}
function log(msg, at) {
  const f = $("feed"), li = document.createElement("li");
  li.innerHTML = `<time>${esc(hm(at || now()))}</time><span>${esc(msg)}</span>`;
  f.prepend(li);
  while (f.children.length > 3) f.lastChild.remove();
}
/** The last three starts and ends among the runs of the members. */
function feedFromRuns() {
  $("feed").textContent = "";
  const ev = [];
  for (const m of M) for (const r of m.runs) {
    ev.push([r.started, t("{who} starts: {task}", { who: m.name || m.id, task: r.task })]);
    if (r.state !== "running" && r.ended) ev.push([r.ended, t("{who} finished: {task}", { who: m.name || m.id, task: r.task })]);
  }
  ev.sort((a, b) => a[0] - b[0]).slice(-3).forEach(([at, msg]) => log(msg, at));
}

/* ---------- side sheet ---------- */
const dlg = $("fiche");
function openFiche(id) {
  openId = id; renderFiche(true);
  if (!dlg.open) dlg.showModal();
  try { history.replaceState(null, "", "#" + id); } catch (e) { /* sandboxed */ }
}
dlg.addEventListener("close", () => { openId = null; try { history.replaceState(null, "", location.pathname + location.search); } catch (e) { /* sandboxed */ } });
dlg.addEventListener("click", e => { if (e.target === dlg) dlg.close(); });
const planUrl = () => /^https?:[/][/]/.test(TEAM.plan_url || "") ? TEAM.plan_url : "";
const planLink = id => planUrl() ? `<a class="id" href="${esc(planUrl().replace(/\/$/, ""))}/#/t/${esc(id)}">${esc(id)}</a>` : `<span class="id">${esc(id)}</span>`;
const STATE = { in_progress: "In progress", todo: "To do", waiting: "Waiting" };
let shown = "";
function renderFiche(force) {
  const m = M.find(x => x.id === openId); if (!m) return;
  const sig = JSON.stringify([stateOf(m), m.cur.map(c => c.k), m.done.map(d => d.key + d.v[1]), m.queue.map(q => q.id + q.state), m.rate]);
  if (!force && sig === shown) return;
  shown = sig;
  const y = dlg.scrollTop, focused = dlg.contains(document.activeElement);
  const s = stateOf(m);
  const nowItems = s === "work" ? m.cur.map(c => `<li>${c.ticket ? planLink(c.ticket) + " " : ""}${esc(c.task)}</li>`).join("") : `<li>${esc(taskLine(m))}</li>`;
  const done = m.done.length ? `<ol class="list">${m.done.slice(0, 40).map(d => `<li class="${seen.has(d.key) ? "" : "fresh"}">
      <p class="t">${esc(d.title)}</p>
      <p class="m">${d.ticket ? planLink(d.ticket) : ""}<span>${esc(when(d.t))}</span>${d.meta ? `<span>${esc(d.meta)}</span>` : ""}</p>
      <span class="tags"><span class="v v-${d.v[0]}">${esc(d.v[1])}</span></span></li>`).join("")}</ol>`
    : `<p class="empty">${esc(t("No run in the last 7 days."))}</p>`;
  m.done.forEach(d => seen.add(d.key));
  const queue = m.queue.length ? `<ol class="list q">${m.queue.map((q, i) => `<li><span class="no">${i + 1}</span><span class="body">
      <p class="t">${esc(q.title)}</p><p class="m">${planLink(q.id)}<span>${esc(t(STATE[q.state] || q.state))}</span></p></span></li>`).join("")}</ol>`
    : `<p class="empty">${esc(m.empty_queue || t("Nothing in its queue."))}</p>`;
  const rate = m.rate != null ? `<p class="rate">${esc(t("{x} per hour over 30 days", { x: money(m.rate) }))}</p>` : "";
  $("ficheIn").innerHTML = `
    <div class="fhead s-${s}"><span id="fpt"></span>
      <div class="who"><h2 id="fname">${esc(m.name || m.id)}</h2><p class="rl">${esc(m.role || "")}</p>${m.motto ? `<p class="motto">« ${esc(m.motto)} »</p>` : ""}${rate}</div>
      <button class="close" type="button" id="closeBtn" aria-label="${esc(t("Close"))}"><svg viewBox="0 0 18 18" aria-hidden="true"><path d="M4 4L14 14M14 4L4 14" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></svg></button>
    </div>
    <div class="now s-${s}"><span class="st">${esc(statusLine(m))}</span><ul>${nowItems}</ul></div>
    <div class="cols">
      <section><h3>${esc(t("What it did"))}</h3><p class="hint">${esc(t("Latest first, with the verdict of the check."))}</p>${done}</section>
      <section><h3>${esc(t("In its queue"))}</h3><p class="hint">${esc(t("What it will take next, in order."))}</p>${queue}</section>
    </div>`;
  $("fpt").replaceWith(portrait(m));
  $("closeBtn").addEventListener("click", () => dlg.close());
  dlg.scrollTop = y;
  if (focused) $("closeBtn").focus();
}

/* ---------- live ---------- */
let timer = null;
async function refresh(first) {
  try {
    const since = Math.floor(nowS() - WINDOW);
    const [team, runs, rates] = await Promise.all([get("api/team"), get("api/runs?since=" + since), get("api/agent-rates")]);
    TEAM = team; RUNS = runs;
    RATES = Object.fromEntries((rates.agents || []).map(a => [a.agent, a]));
    document.body.classList.remove("offline"); $("alert").hidden = true;
    if (mode === "replay") return;
    const before = new Map(M.flatMap(m => m.runs.map(r => [rid(r), r.state])));
    build();
    if (first) { buildCards(); feedFromRuns(); M.forEach(m => m.done.forEach(d => seen.add(d.key))); }
    else {
      for (const m of M) for (const r of m.runs.slice().reverse()) {
        const was = before.get(rid(r));
        if (was === undefined) log(t("{who} starts: {task}", { who: m.name || m.id, task: r.task }), r.started);
        if (r.state !== "running" && was !== r.state && r.ended) log(t("{who} finished: {task}", { who: m.name || m.id, task: r.task }), r.ended);
      }
      if (M.some(m => !$("card-" + m.id)) || document.querySelectorAll(".agent").length !== M.length) buildCards(); else M.forEach(updateCard);
    }
    summary();
    if (openId) renderFiche();
  } catch (e) {
    document.body.classList.add("offline");
    $("alert").textContent = t("Cairn Plan does not answer. The page tries again in 10 seconds.");
    $("alert").hidden = false;
  }
  chip();
}

/* ---------- replay of the last 24 hours ---------- */
let events = [], rTimer = null, R0 = 0, R1 = 0;
function replay() {
  const end = nowS(), start = end - 86400;
  // runs started in the window only: a session left open for days is not "working" all along
  const runs = M.flatMap(m => m.runs.filter(r => r.started >= start || (r.ended || 0) >= start).map(r => ({ m, r })));
  if (!runs.length) { log(t("No run in the last 24 hours: nothing to replay.")); return; }
  mode = "replay"; document.body.classList.add("replaying");
  R0 = Math.max(start, Math.min(...runs.map(x => x.r.started))) - 60;
  const fromStart = r => r.started >= start; R1 = end; clock = R0;
  M.forEach(m => { m.cur = []; m.stale = []; m.waitRun = null; m.done = m.done.filter(d => d.t < R0); });
  events = [];
  for (const { m, r } of runs) {
    if (fromStart(r)) events.push({ at: r.started, f: () => { m.cur.push({ k: rid(r), task: r.task, ticket: r.ticket, since: r.started }); log(t("{who} starts: {task}", { who: m.name || m.id, task: r.task })); updateCard(m); } });
    if (r.state !== "running" && r.ended) events.push({ at: r.ended, f: () => {
      m.cur = m.cur.filter(c => c.k !== rid(r)); m.done.unshift(item(r)); seen.delete(rid(r));
      log(t("{who} finished: {task}", { who: m.name || m.id, task: r.task })); updateCard(m); } });
  }
  events.sort((a, b) => a.at - b.at);
  $("feed").textContent = "";
  log(t("Replay of the last 24 hours, in one minute."));
  $("replaylabel").textContent = t("Stop the replay");
  $("prog").hidden = false;
  M.forEach(updateCard); summary(); if (openId) renderFiche();
  const step = (R1 - R0) / 600;
  rTimer = setInterval(() => {
    clock += step;
    while (events.length && events[0].at <= clock) events.shift().f();
    $("progbar").style.width = Math.min(100, (clock - R0) / (R1 - R0) * 100) + "%";
    summary(); tick(); if (openId) renderFiche();
    if (clock >= R1) stopReplay();
  }, 100);
}
function stopReplay() {
  clearInterval(rTimer); rTimer = null; mode = "live";
  document.body.classList.remove("replaying"); $("prog").hidden = true; $("progbar").style.width = "0";
  $("replaylabel").textContent = t("Replay the last 24 hours");
  build(); M.forEach(updateCard); summary(); feedFromRuns(); if (openId) renderFiche(); chip();
}

/* ---------- start ---------- */
(async function start() {
  try { ME = await get("api/me"); LANG = pickLang(ME.lang); } catch (e) { /* the refresh shows the error */ }
  document.documentElement.lang = LANG;
  document.title = t("The team");
  $("title").textContent = t("The team");
  $("host").textContent = location.host;
  $("replaylabel").textContent = t("Replay the last 24 hours");
  $("foot").textContent = t("Each agent reports its runs to Cairn Plan. The queue lists the open tickets assigned to it. The page reads again every 10 seconds.");
  $("replay").addEventListener("click", () => mode === "replay" ? stopReplay() : replay());
  await refresh(true);
  if (planUrl()) $("brand").href = planUrl();
  const h = decodeURIComponent(location.hash.slice(1)); if (M.some(m => m.id === h)) openFiche(h);
  const again = () => { if (mode === "live" && document.visibilityState === "visible") refresh(false); };
  timer = setInterval(again, REFRESH);
  document.addEventListener("visibilitychange", again);
  setInterval(() => { if (mode === "live") tick(); }, 1000);
})();
