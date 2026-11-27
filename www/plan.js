// cairn-plan page. Text is set with textContent only: nothing typed by a
// member is ever read as HTML. The server decides who may do what; the page
// only greys out what would be refused.
"use strict";

const $ = s => document.querySelector(s);

function el(tag, props, ...kids) {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(props || {})) {
    if (k === "class") e.className = v;
    else if (k.startsWith("on")) e.addEventListener(k.slice(2), v);
    else if (k in e) e[k] = v;
    else e.setAttribute(k, v);
  }
  for (const k of kids.flat()) if (k != null && k !== false && k !== "") e.append(k);
  return e;
}

// A comment is plain text: its web addresses become links, nothing else is read as markup.
function linked(text) {
  const out = [];
  let last = 0;
  for (const m of text.matchAll(/https?:\/\/[^\s<>"]*[^\s<>".,;:!?'")\]]/g)) {
    out.push(text.slice(last, m.index), el("a", { href: m[0], target: "_blank", rel: "noopener noreferrer" }, m[0]));
    last = m.index + m[0].length;
  }
  out.push(text.slice(last));
  return out;
}

// Mermaid is a large file (about 3 MB): fetched once, by the first page that shows a
// ```mermaid block. Same origin, pinned version (the name of the file says which).
let mermaidReady = null;
function loadMermaid() {
  mermaidReady ??= new Promise((resolve, reject) => {
    document.head.append(el("script", { src: "mermaid-11.12.0.min.js", onload: resolve,
      onerror: () => { mermaidReady = null; reject(new Error("mermaid.js not loaded")); } }));
  });
  return mermaidReady;
}

let diagrams = 0;
const darkScheme = matchMedia("(prefers-color-scheme: dark)");

// Draws the source into box as SVG. "strict": no HTML and no click in a diagram
// (the text comes from members). Any failure leaves the source as text, plus a short message.
async function drawDiagram(box, source) {
  const pre = el("pre", {}, source), id = "diagram-" + (++diagrams);
  try {
    await loadMermaid();
    mermaid.initialize({ startOnLoad: false, securityLevel: "strict", suppressErrorRendering: true, theme: darkScheme.matches ? "dark" : "default" });
    const { svg } = await mermaid.render(id, source);
    const view = el("div", { class: "diagram" });
    view.innerHTML = svg;   // SVG built and sanitised by Mermaid itself (strict), never typed text
    box.replaceChildren(view);
    box.classList.remove("failed");
  } catch (e) {
    // What this failed render can leave. Only its own: the other diagrams of the page
    // may be drawing at the same moment, in their own temporary element.
    document.getElementById("d" + id)?.remove();
    box.replaceChildren(el("p", { class: "meta" }, t("This diagram could not be drawn. Source shown instead.")), pre);
    box.classList.add("failed");
  }
}

// The diagrams on screen follow the light or dark scheme of the system.
darkScheme.addEventListener("change", () => {
  document.querySelectorAll(".mermaid").forEach(box => drawDiagram(box, box.dataset.source));
});

// A description or a comment: fenced blocks (```) are shown as they are, each
// with a button that copies it (a command to paste); the rest is linked() text.
// A ```mermaid block is drawn as a diagram; its button copies the source.
function rich(text) {
  const out = [];
  const plain = s => { s = s.replace(/^\n+|\n+$/g, ""); if (s) out.push(el("p", { class: "body" }, linked(s))); };
  let last = 0;
  for (const m of text.matchAll(/```([^\n]*)\n([\s\S]*?)\n?```/g)) {
    plain(text.slice(last, m.index));
    const code = m[2];
    const draw = m[1].trim().toLowerCase() === "mermaid";
    const shown = draw ? el("div", { class: "mermaid" }) : el("pre", {}, code);
    if (draw) {
      shown.dataset.source = code;
      shown.append(el("pre", {}, code));   // shown until the drawing is ready
      drawDiagram(shown, code);
    }
    out.push(el("div", { class: "code" }, shown, el("button", { type: "button", onclick: act(async () => {
      try {
        await navigator.clipboard.writeText(code);
        toast(t("Copied"));
      } catch (e) {   // clipboard refused (permission, embedded view): selected, for Ctrl+C
        getSelection().selectAllChildren(shown);
        toast(t("Selected: copy it with Ctrl+C."));
      }
    }) }, t("Copy"))));
    last = m.index + m[0].length;
  }
  plain(text.slice(last));
  return out;
}

// Sign-in lost (session ended at the proxy): the proxy answers with a
// redirect to its sign-in page, which a background request cannot follow.
// The page is NOT reloaded: that would wipe what someone is typing. A banner
// says so and opens the sign-in in a new tab; drafts (below) keep the text.
const ENDED = "Your session has ended. Your text is kept: sign in again, then send again.";
function signedIn(r) {
  const gone = r.type === "opaqueredirect" || r.status === 401;
  const b = $("#lost");
  if (gone) b.replaceChildren(el("span", {}, t("Your session has ended. What you typed is kept in this browser.")), " ",
    el("a", { href: location.pathname, target: "_blank" }, t("Sign in again (new tab)")), " ",
    el("small", {}, t("then come back to this tab and send again.")));
  b.hidden = !gone;
  return !gone;
}

// What someone types in a ticket is kept in this browser while they type (one
// key per ticket and field): a lost session, a reload or a closed tab loses
// nothing. Put back when the page is drawn again, removed once it is sent.
// A draft keeps the text it started from: if that text changed since (someone
// else edited the description), the draft is dropped rather than writing over
// the newer text. A field that cannot be edited drops its draft.
// Returns the "Draft restored" mark when there was one, else null.
function draft(box, key) {
  key = "cairn-plan-draft:" + key;
  const drawn = box.value;
  const store = f => { try { return f(localStorage); } catch (e) { return null; } };   // storage blocked or full
  box.addEventListener("input", () => store(s => box.value === drawn ? s.removeItem(key) : s.setItem(key, JSON.stringify({ from: drawn, text: box.value }))));
  box.sent = () => store(s => s.removeItem(key));
  let kept = null;
  try { kept = JSON.parse(store(s => s.getItem(key))); } catch (e) { /* not a draft */ }
  if (box.disabled || !kept || kept.from !== drawn || kept.text === drawn) { box.sent(); return null; }
  box.value = kept.text;
  return el("small", { class: "draft" }, t("Draft restored"));
}

async function api(path, body) {
  const r = await fetch(path, body === undefined ? { redirect: "manual" } : {
    redirect: "manual", method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body),
  });
  if (!signedIn(r)) throw new Error(t(ENDED));
  const j = await r.json().catch(() => ({ error: "request failed" }));
  if (!r.ok) throw new Error(t(j.error || "request failed"));
  return j;
}

// Sends one file as it is (not JSON). The server names the limit; checking here spares a long wait.
async function upload(path, file) {
  if (file.size > ME.max_file_mb * 1048576) throw new Error(t("file too large"));
  const r = await fetch(path, { redirect: "manual", method: "POST", headers: { "Content-Type": "application/octet-stream" }, body: file });
  if (!signedIn(r)) throw new Error(t(ENDED));
  const j = await r.json().catch(() => ({ error: "request failed" }));
  if (!r.ok) throw new Error(t(j.error || "request failed"));
  return j;
}

let toastTimer;
function toast(msg) {
  const b = $("#toast");
  b.textContent = msg;
  b.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { b.hidden = true; }, 4000);
}

// Wraps a handler: stops the form, shows a refusal instead of failing silently.
const act = fn => async ev => {
  if (ev) ev.preventDefault();
  try { await fn(ev); } catch (e) { toast(e.message); }
};

const STATE = { backlog: "Proposed", todo: "To do", in_progress: "In progress", waiting: "Waiting", done: "Done", cancelled: "Cancelled" };
const STATE_CLASS = { backlog: "unk", todo: "todo", in_progress: "warn", waiting: "wait", done: "ok", cancelled: "off" };
const PRIORITY = { none: "No priority", low: "Low", medium: "Medium", high: "High", urgent: "Urgent" };
const ROLE = { reader: "Reader", contributor: "Contributor", owner: "Owner", admin: "Administrator" };
const FIELD = { title: "Title", state: "State", priority: "Priority", assignee: "Assignee", start_date: "Start", end_date: "End", milestone: "Milestone", agent: "Agent", wake_date: "Wake up" };

let ME = { states: [], priorities: [] };
let FILTER = "open";

const isOwner = role => role === "owner" || role === "admin";
const day = s => s ? new Date(s + "T00:00:00").toLocaleDateString(LOCALE(), { dateStyle: "medium" }) : "";
// Time someone typed: "1h30", "1h 30m", "45m", "1,5h", or a number of minutes. 0: not understood.
function minutes(s) {
  const m = s.trim().toLowerCase().replace(",", ".").match(/^(?:(\d+(?:\.\d+)?)\s*h)?\s*(?:(\d+)\s*(?:m|min)?)?$/);
  return m && (m[1] || m[2]) ? Math.round(parseFloat(m[1] || 0) * 60 + parseInt(m[2] || 0, 10)) : 0;
}
// Time worked, from minutes: "45 min", "2 h", "1 h 30".
const span = min => min < 60 ? t("{m} min", { m: min })
  : min % 60 ? t("{h} h {m}", { h: Math.floor(min / 60), m: String(min % 60).padStart(2, "0") }) : t("{h} h", { h: min / 60 });
// Time on the clock, from seconds: minutes, then hours, then days and hours.
const clock = sec => sec < 86400 ? span(Math.max(1, Math.round(sec / 60)))
  : t("{d} d {h} h", { d: Math.floor(sec / 86400), h: Math.floor(sec % 86400 / 3600) });
const stamp = sec => new Date(sec * 1000).toLocaleString(LOCALE(), { dateStyle: "medium", timeStyle: "short" });
const pill = state => el("span", { class: "pill " + STATE_CLASS[state] }, t(STATE[state] || state));
// Who someone is at a glance: their photo, else their initials on a disc, one colour per
// person; a square for an agent. ME.photos says who has a photo (login -> version): the
// page never asks for a photo that does not exist. size: "lg" or "xl" (the profile page).
function avatar(login, size) {
  const shape = ((ME.agents || []).includes(login) || login.endsWith("·claude") ? " bot" : "") + (size ? " " + size : "");
  const parts = login.replace(/·claude$/, "").split(/[-._ ]+/).filter(Boolean);
  const text = ((parts[0] || "?")[0] + (parts[1] ? parts[1][0] : (parts[0] || "")[1] || "")).toUpperCase();
  let h = 0;
  for (const ch of login) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
  const initials = el("span", { class: "avatar a" + (h % 7 + 1) + shape, "aria-hidden": "true" }, text);
  const version = (ME.photos || {})[login];
  return version ? el("img", { class: "avatar" + shape, alt: "", loading: "lazy", onerror: ev => ev.target.replaceWith(initials),
    src: "api/avatars/" + encodeURIComponent(login) + "?v=" + version }) : initials;
}
const person = login => el("span", { class: "person" }, avatar(login), login);
const field = (label, control, hint) => el("label", { class: "field" }, el("span", {}, t(label)), control, hint && el("small", {}, t(hint)));

function select(values, current, label, props) {
  return el("select", props, values.map(v => el("option", { value: v, selected: v === current }, label(v))));
}

// Space of a project: free text, with the spaces already in use as suggestions.
function spaceInput(ps, value, disabled) {
  const input = el("input", { maxLength: 60, value: value || "", disabled: !!disabled });
  input.setAttribute("list", "spaces");   // "list" is read-only as a property
  const names = [...new Set(ps.map(p => p.space).filter(Boolean))];
  return [input, el("datalist", { id: "spaces" }, names.map(s => el("option", { value: s })))];
}

function show(...nodes) {
  $("#view").replaceChildren(...nodes.flat().filter(Boolean));
  window.scrollTo(0, 0);
  if (LIVE && !LIVE.form) LIVE.form = fields();   // the fields as drawn, to tell a draft from them
}

// The space being worked in: one company or area at a time. The menu and
// the home page only show its projects; opening a project switches to its space.
let SPACE = null;
try { SPACE = localStorage.getItem("cairn-plan-space"); } catch (e) { /* storage blocked */ }
// Whether the list of projects is folded away in the menu.
let FOLDED = false;
try { FOLDED = localStorage.getItem("cairn-plan-folded") === "1"; } catch (e) { /* storage blocked */ }
function setSpace(s) {
  SPACE = s;
  try { localStorage.setItem("cairn-plan-space", s); } catch (e) { /* storage blocked */ }
}

async function nav() {
  const [ps, mine] = await Promise.all([api("api/projects"), api("api/mine")]);
  const asked = mine.filter(k => k.state === "waiting").length;
  const spaces = [...new Set(ps.map(p => p.space))];   // sorted by the server, unfiled last
  if (!spaces.includes(SPACE)) SPACE = spaces.length ? spaces[0] : "";
  const here = location.hash.split("/")[2] || "";
  const hereKey = location.hash.startsWith("#/t/") ? here.slice(0, here.lastIndexOf("-")) : here;
  $("#nav").replaceChildren(...[
    // what waits on me, in every space: the count is the questions asked to me
    el("a", { class: "bell", href: "#/m", ...(location.hash.startsWith("#/m") ? { "aria-current": "page" } : {}) },
      t("For me"), asked > 0 && el("span", { class: "pill count" }, String(asked))),
    el("a", { href: "#/q", ...(location.hash.startsWith("#/q") ? { "aria-current": "page" } : {}) }, t("Search")),
    // the space is always named; a switch only when there is a choice
    spaces.length > 1 ? select(spaces, SPACE, s => s || t("No space"), {
      class: "space-switch", "aria-label": t("Space"),
      onchange: ev => { setSpace(ev.target.value); if (location.hash.length > 2) location.hash = "#/"; else route(); },
    }) : SPACE && el("span", { class: "space-name" }, el("small", {}, t("Space")), SPACE),
    // "Projects", then the projects of the space one step in; the arrow folds them away
    el("details", { class: "group", open: !FOLDED, ontoggle: ev => {
      FOLDED = !ev.target.open;
      try { localStorage.setItem("cairn-plan-folded", FOLDED ? "1" : ""); } catch (e) { /* storage blocked */ }
    } },
      el("summary", {}, el("a", { href: "#/", ...(location.hash.length < 3 ? { "aria-current": "page" } : {}) }, t("Projects"))),
      el("div", { class: "subnav" }, ps.filter(p => p.space === SPACE).map(p =>
        el("a", { href: "#/p/" + p.key, ...(p.key === hereKey && !location.hash.endsWith("/time") ? { "aria-current": "page" } : {}) }, p.name)))),
    // what the agents do, in every space
    el("a", { href: "#/a", ...(location.hash.startsWith("#/a") ? { "aria-current": "page" } : {}) }, t("Agents")),
    // the time of the space, then of each project from its page
    ps.length > 0 && el("a", { class: "time-link", href: "#/e/" + encodeURIComponent(SPACE),
      ...(location.hash.startsWith("#/e/") || location.hash.endsWith("/time") ? { "aria-current": "page" } : {}) }, t("Time")),
  ].filter(Boolean));
  return ps;
}

/* ---------- projects ---------- */

async function home() {
  const all = await nav();
  const ps = all.filter(p => p.space === SPACE);
  const key = el("input", { required: true, maxLength: 10, pattern: "[A-Z][A-Z0-9]{1,9}", autocapitalize: "characters" });
  const name = el("input", { required: true, maxLength: 100 });
  const desc = el("input", { maxLength: 2000 });
  const [space, spaceList] = spaceInput(all, SPACE);
  show(
    el("div", { class: "top" }, el("div", {}, el("h1", {}, SPACE || t("Projects")), SPACE && el("p", { class: "meta" }, t("Projects")))),
    !ps.length && el("p", { class: "empty" }, t(ME.admin ? "No project yet. Create the first one below."
      : "You are not a member of any project yet. Ask a project owner to add you.")),
    el("div", { class: "grid tiles" }, ps.map(p => el("a", { class: "card tile", href: "#/p/" + p.key },
      el("strong", {}, p.name),
      el("span", { class: "label" }, p.key + " · " + t(ROLE[p.role])),
      p.description && el("span", {}, p.description),
      el("span", { class: "label" }, tp(p.open, "{n} open ticket", "{n} open tickets") + " · " + t("{n} in total", { n: p.total }))))),
    ME.admin && el("form", { class: "card form", onsubmit: act(async () => {
      const r = await api("api/projects", { key: key.value, name: name.value, description: desc.value, space: space.value });
      location.hash = "#/p/" + r.key;
    }) },
      el("h2", {}, t("New project")),
      el("div", { class: "fields" },
        field("Key", key, "Short code in capitals, used in ticket numbers (SHOP-12)."),
        field("Name", name),
        field("Space", space, "The company or area the project is filed under (optional)."),
        field("Description", desc)),
      spaceList,
      el("button", { class: "primary" }, t("Create the project"))),
  );
}

// How the tickets of a project are shown: kept from one project to the next.
let VIEW = "list";
try { VIEW = localStorage.getItem("cairn-plan-view") === "board" ? "board" : "list"; } catch (e) { /* storage blocked */ }
const FILTERS = { q: "", assignee: "*", priority: "*", milestone: "*" };
const BOARD = ["backlog", "todo", "in_progress", "waiting", "done"];   // "cancelled" joins when it has tickets

/** Progress of a parent: its sub-tickets done out of those not cancelled; nothing for a ticket without any. */
function progress(k) {
  const { done = 0, total = 0 } = k.progress || {};
  if (!total) return null;
  const pct = Math.round(100 * done / total);
  return el("span", { class: "progress", title: t("{done} of {total} sub-tickets done", { done, total }) },
    el("span", { class: "bar", role: "progressbar", "aria-valuemin": "0", "aria-valuemax": String(total), "aria-valuenow": String(done),
      "aria-label": t("Sub-tickets done") }, el("span", { style: "width:" + pct + "%" })),
    el("span", { class: "label" }, done + "/" + total));
}

async function project(key) {
  const p = live("api/projects/" + encodeURIComponent(key), await api("api/projects/" + encodeURIComponent(key)));
  setSpace(p.space);
  const canEdit = p.role !== "reader";
  // a filter on someone who is not in this project would hide everything
  if (FILTERS.assignee !== "*" && FILTERS.assignee && !p.members.some(m => m.login === FILTERS.assignee)) FILTERS.assignee = "*";
  const stones = [...new Set(p.tickets.map(k => k.milestone).filter(Boolean))].sort();
  if (FILTERS.milestone !== "*" && FILTERS.milestone && !stones.includes(FILTERS.milestone)) FILTERS.milestone = "*";
  // a project where no ticket has a priority has no use for that filter
  const ranked = p.tickets.some(k => k.priority !== "none");
  if (!ranked) FILTERS.priority = "*";
  const title = el("input", { required: true, maxLength: 200, placeholder: t("Title of the ticket"), "aria-label": t("Title of the ticket") });
  const about = el("textarea", { rows: 4, maxLength: 20000, "aria-label": t("Description"),
    placeholder: t("What is needed, for whom, and how we will know it is done.") });
  const aboutMark = draft(about, "new-" + key + ":description");
  // "" : a proposal; "todo": an owner creates and approves in one gesture
  const add = state => act(async () => {
    if (!title.reportValidity()) return;
    const r = await api("api/projects/" + encodeURIComponent(key) + "/tickets", { title: title.value, description: about.value, ...(state && { state }) });
    about.sent();
    location.hash = "#/t/" + r.id;
  });
  const cell = (label, ...kids) => el("td", { "data-label": t(label) }, kids);
  const prio = k => k.priority !== "none" && el("span", { class: "pill prio-" + k.priority }, t(PRIORITY[k.priority]));
  const agent = k => k.agent && el("span", { class: "pill agent" }, k.agent);
  const tickets = el("div", {}), count = el("span", { class: "label" });

  // A column that no ticket shown fills is left out, like the milestone.
  const table = shown => {
    const any = f => shown.some(k => k[f] && k[f] !== "none");
    const cols = { priority: any("priority"), assignee: any("assignee"), start: any("start_date"), end: any("end_date") };
    const heads = ["Ticket", "Title", "State", stones.length > 0 && "Milestone", cols.priority && "Priority", cols.assignee && "Assignee",
      cols.start && "Start", cols.end && "End"].filter(Boolean);
    return el("div", { class: "card table-wrap" }, shown.length ? el("table", {},
      el("thead", {}, el("tr", {}, heads.map(h => el("th", {}, t(h))))),
      el("tbody", {}, shown.map(k => el("tr", {},
        cell("Ticket", el("a", { href: "#/t/" + k.id }, k.id)),
        cell("Title", el("a", { href: "#/t/" + k.id }, k.title), k.parent && el("span", { class: "label" }, " · " + k.parent), " ", agent(k), progress(k)),
        cell("State", pill(k.state)),
        stones.length > 0 && cell("Milestone", k.milestone),
        cols.priority && cell("Priority", prio(k)),
        cols.assignee && cell("Assignee", k.assignee && person(k.assignee)),
        cols.start && cell("Start", day(k.start_date)),
        cols.end && cell("End", day(k.end_date))))))
      : el("p", { class: "empty" }, t("No ticket here.")));
  };

  // A card dropped on a column asks for that state; the server says who may.
  const board = found => el("div", { class: "board" },
    BOARD.concat(found.some(k => k.state === "cancelled") ? ["cancelled"] : []).map(s => {
      const items = found.filter(k => k.state === s);
      return el("section", { class: "kcol",
        ondragover: ev => ev.preventDefault(),
        ondrop: act(async ev => {
          const id = ev.dataTransfer.getData("text/plain");
          const k = p.tickets.find(x => x.id === id);
          if (!k || k.state === s) return;
          await api("api/tickets/" + encodeURIComponent(id), { state: s });
          await project(key);
        }) },
        el("h2", {}, pill(s), el("span", { class: "label" }, String(items.length))),
        items.map(k => el("a", { class: "kcard", href: "#/t/" + k.id, draggable: canEdit,
          ondragstart: ev => { ev.dataTransfer.setData("text/plain", k.id); ev.dataTransfer.effectAllowed = "move"; } },
          el("span", { class: "label" }, k.id),
          el("strong", {}, k.title),
          progress(k),
          el("span", { class: "kmeta" }, prio(k), agent(k), k.assignee && el("span", { class: "label" }, person(k.assignee)),
            k.end_date && el("span", { class: "label" }, day(k.end_date))))));
    }));

  const stateFilter = el("label", { class: "inline" }, t("Show"), select(["open", "all", ...ME.states], FILTER,
    v => v === "open" ? t("Open tickets") : v === "all" ? t("All tickets") : t(STATE[v]),
    { onchange: ev => { FILTER = ev.target.value; draw(); } }));
  const viewButton = (v, label) => el("button", { type: "button", "aria-pressed": String(VIEW === v), onclick: () => {
    VIEW = v;
    try { localStorage.setItem("cairn-plan-view", v); } catch (e) { /* storage blocked */ }
    project(key);
  } }, t(label));
  const hint = el("small", {}, t("Drag a card to another column to change its state. Only a project owner approves a proposal."));

  function draw() {
    const q = FILTERS.q.trim().toLowerCase();
    const found = p.tickets.filter(k => (!q || (k.id + " " + k.title).toLowerCase().includes(q))
      && (FILTERS.assignee === "*" || k.assignee === FILTERS.assignee)
      && (FILTERS.priority === "*" || k.priority === FILTERS.priority)
      && (FILTERS.milestone === "*" || k.milestone === FILTERS.milestone));
    const shown = VIEW === "board" ? found
      : found.filter(k => FILTER === "all" || (FILTER === "open" ? k.state !== "done" && k.state !== "cancelled" : k.state === FILTER));
    stateFilter.hidden = VIEW === "board";
    hint.hidden = VIEW !== "board" || !canEdit;
    count.textContent = tp(shown.length, "{n} ticket", "{n} tickets");
    tickets.replaceChildren(VIEW === "board" ? board(found) : table(shown));
  }

  show(
    el("div", { class: "top" },
      el("div", {}, el("h1", {}, p.name), el("p", { class: "meta" }, [p.space, p.key, t(ROLE[p.role]), p.description].filter(Boolean).join(" · "))),
      el("div", { class: "row" },
        el("a", { class: "button", href: "#/p/" + p.key + "/time" }, t("Time")),
        el("a", { class: "button", href: "#/p/" + p.key + "/members" }, tp(p.members.length, "{n} member", "{n} members")))),
    el("div", { class: "row toolbar" },
      el("span", { class: "toggle" }, viewButton("list", "List"), viewButton("board", "Board")),
      el("input", { type: "search", value: FILTERS.q, placeholder: t("Search a ticket"), "aria-label": t("Search a ticket"),
        oninput: ev => { FILTERS.q = ev.target.value; draw(); } }),
      stateFilter,
      select(["*", "", ...p.members.map(m => m.login)], FILTERS.assignee, v => v === "*" ? t("Everyone") : v || t("Unassigned"),
        { "aria-label": t("Assignee"), onchange: ev => { FILTERS.assignee = ev.target.value; draw(); } }),
      ranked && select(["*", ...ME.priorities], FILTERS.priority, v => v === "*" ? t("All priorities") : t(PRIORITY[v]),
        { "aria-label": t("Priority"), onchange: ev => { FILTERS.priority = ev.target.value; draw(); } }),
      stones.length > 0 && select(["*", "", ...stones], FILTERS.milestone, v => v === "*" ? t("All milestones") : v || t("No milestone"),
        { "aria-label": t("Milestone"), class: "wide-select", onchange: ev => { FILTERS.milestone = ev.target.value; draw(); } }),
      count),
    tickets,
    hint,
    canEdit && el("form", { class: "card form", onsubmit: add("") },
      el("h2", {}, t("New ticket")),
      title,
      about, aboutMark,
      el("div", { class: "row" },
        isOwner(p.role) && el("button", { type: "button", class: "approve", onclick: add("todo") }, t("Create and approve")),
        el("button", { class: isOwner(p.role) ? "" : "primary" }, t(isOwner(p.role) ? "Add as a proposal" : "Add the ticket"))),
      el("small", {}, t(isOwner(p.role) ? "Create and approve: the ticket is ready to be done at once. A proposal waits for your approval."
        : "A new ticket starts as a proposal; a project owner approves it."))),
  );
  draw();
}

async function members(key) {
  const p = await api("api/projects/" + encodeURIComponent(key));
  const owner = isOwner(p.role);
  setSpace(p.space);
  const set = (login, role) => act(async () => {
    await api("api/projects/" + encodeURIComponent(key) + "/members", { login, role });
    await members(key);
  });
  const login = el("input", { required: true, maxLength: 64, pattern: "[a-z0-9._\\-]+", autocapitalize: "none" });
  const role = select(["reader", "contributor", "owner"], "contributor", v => t(ROLE[v]));
  const pname = el("input", { required: true, maxLength: 100, value: p.name });
  const pdesc = el("input", { maxLength: 2000, value: p.description });
  const [pspace, spaceList] = spaceInput(owner ? await api("api/projects") : [], p.space, !ME.admin);
  show(
    el("div", { class: "top" },
      el("div", {}, el("h1", {}, t("Members of {name}", { name: p.name })),
        el("p", { class: "meta" }, t("Reader: reads and comments. Contributor: creates and edits tickets. Owner: approves tickets and manages members."))),
      el("div", { class: "row" },
        el("a", { class: "button", href: "api/projects/" + encodeURIComponent(p.key) + "/export", download: p.key + ".json" }, t("Export")),
        el("a", { class: "button", href: "#/p/" + p.key }, t("Back to the project")))),
    el("div", { class: "card table-wrap" }, el("table", {},
      el("thead", {}, el("tr", {}, ["Login", "Name", "Role", ""].map(h => el("th", {}, h && t(h))))),
      el("tbody", {}, p.members.map(m => el("tr", {},
        el("td", { "data-label": t("Login") }, person(m.login)),
        el("td", { "data-label": t("Name") }, m.name),
        el("td", { "data-label": t("Role") }, owner
          ? select(["reader", "contributor", "owner"], m.role, v => t(ROLE[v]), { onchange: ev => set(m.login, ev.target.value)() })
          : t(ROLE[m.role])),
        el("td", { class: "actions" }, owner && el("button", { type: "button", onclick: set(m.login, null) }, t("Remove")))))))),
    owner && el("form", { class: "card form", onsubmit: act(async () => {
      await api("api/projects/" + encodeURIComponent(key) + "/members", { login: login.value, role: role.value });
      await members(key);
    }) },
      el("h2", {}, t("Add a member")),
      el("div", { class: "fields" },
        field("Login", login, "The name the person signs in with, in lower case."),
        field("Role", role)),
      el("button", { class: "primary" }, t("Add"))),
    owner && el("form", { class: "card form", onsubmit: act(async () => {
      await api("api/projects/" + encodeURIComponent(key), { name: pname.value, description: pdesc.value, space: pspace.value });
      await members(key);
      await nav();
      toast(t("Saved"));
    }) },
      el("h2", {}, t("Project settings")),
      el("div", { class: "fields" },
        field("Name", pname),
        field("Space", pspace, ME.admin ? "The company or area the project is filed under (optional)." : "Only an administrator can move a project to another space."),
        field("Description", pdesc)),
      spaceList,
      el("button", { class: "primary" }, t("Save"))),
  );
}

/* ---------- for me ---------- */

// How long a ticket has been in its state: what waits longest shows it.
function since(sec) {
  const h = Math.floor((Date.now() / 1000 - sec) / 3600);
  return h < 1 ? t("for less than an hour") : h < 24 ? tp(h, "for {n} hour", "for {n} hours") : tp(Math.floor(h / 24), "for {n} day", "for {n} days");
}

async function mine() {
  const ks = live("api/mine", await api("api/mine"));
  // under a question: the last thing someone else said, which may change what is asked
  const row = k => el("li", {},
    el("a", { href: "#/t/" + k.id },
      el("strong", {}, k.id + " · " + k.title),
      el("span", { class: "label" }, [k.space, k.project_name,
        t("by {author}", { author: k.state === "waiting" && k.assignee !== ME.login ? k.assignee : k.author }), since(k.since)].filter(Boolean).join(" · ")),
      k.state === "waiting" && k.last.author && k.last.author !== ME.login
        && el("span", { class: "excerpt clamp" }, t("{author}: {text}", { author: k.last.author, text: k.last.body }))),
    pill(k.state));
  const asked = ks.filter(k => k.state === "waiting"), rest = ks.filter(k => k.state !== "waiting");
  show(
    el("div", { class: "top" }, el("div", {}, el("h1", {}, t("For me")),
      el("p", { class: "meta" }, t("What waits on you, in every space: the tickets assigned to you and the questions on tickets you approved.")))),
    el("h2", {}, t("Waiting for your answer")),
    el("div", { class: "card" }, asked.length ? el("ul", { class: "list notices" }, asked.map(row))
      : el("p", { class: "empty" }, t("Nothing waits for you."))),
    rest.length > 0 && el("h2", {}, t("Also assigned to you")),
    rest.length > 0 && el("div", { class: "card" }, el("ul", { class: "list notices" }, rest.map(row))),
  );
}

/* ---------- agent ---------- */

// What the agent says of itself, for an administrator: one sentence, the
// day's budget of each role, then its last lines. Followed like a ticket.
async function agentPage() {
  const a = live("api/agent", await api("api/agent"));
  const head = el("div", { class: "top" }, el("div", {}, el("h1", {}, t("Agent")),
    el("p", { class: "meta" }, t("What the agent that takes approved tickets is doing today."))),
    el("a", { class: "button", href: "#/s" }, t("Settings")));
  if (!a.set) return show(head, el("p", { class: "note" }, t("No agent is set: give its webhook address and secret in the settings.")));
  if (a.error) return show(head, el("p", { class: "note" }, t("The agent does not answer: {error}", { error: a.error })));
  const stopped = a.budgets.filter(b => b.paused || b.usd >= b.max_usd);
  show(head,
    el("p", { class: "agent-now" }, a.ticket ? [t("The agent works on"), " ", el("a", { href: "#/t/" + a.ticket }, a.ticket), "."]
      : a.queued > 0 ? tp(a.queued, "{n} ticket waits for the agent.", "{n} tickets wait for the agent.") : t("The agent has no ticket in hand.")),
    stopped.length > 0 && el("p", { class: "note" }, t("Stopped until the next window: {roles}.", { roles: stopped.map(b => b.role).join(", ") })),
    el("h2", {}, t("Today")),
    el("div", { class: "card" }, el("ul", { class: "list" }, a.budgets.map(b => el("li", {},
      el("span", {}, el("strong", {}, b.role), b.paused && el("span", { class: "label" }, " · " + b.paused)),
      el("span", { class: "label" }, tp(b.tickets, "{n} ticket", "{n} tickets") + " · " + dollars(b.usd) + " / " + dollars(b.max_usd) + (b.window === "night" ? " · " + t("night") : "")))))),
    el("h2", {}, t("Last lines")),
    el("div", { class: "card" }, a.recent.length ? el("ul", { class: "list notices" }, a.recent.map(l => el("li", {},
      el("span", { class: "excerpt" }, l.text), el("span", { class: "label" }, stamp(l.at)))))
      : el("p", { class: "empty" }, t("Nothing yet."))));
}

/* ---------- search ---------- */

// Every ticket of the projects this person sees: id, title, description, comments.
// The text typed is kept from one visit of the page to the next.
let QUERY = "";
async function search() {
  const box = el("input", { type: "search", value: QUERY, maxLength: 100, placeholder: t("A word, a ticket id, a sentence"), "aria-label": t("Search") });
  const found = el("div", {});
  let last = 0;
  const run = async () => {
    QUERY = box.value;
    const mine = ++last;
    if (QUERY.trim().length < 2) return found.replaceChildren(el("p", { class: "empty" }, t("Type two characters or more.")));
    let ks;
    try { ks = await api("api/search?q=" + encodeURIComponent(QUERY.trim())); }
    catch (e) { return found.replaceChildren(el("p", { class: "note" }, e.message)); }
    if (mine !== last) return;   // a later search is on its way
    found.replaceChildren(...[
      el("div", { class: "card" }, ks.length ? el("ul", { class: "list notices" }, ks.map(k => el("li", {},
        el("a", { href: "#/t/" + k.id },
          el("strong", {}, k.id + " · " + k.title),
          el("span", { class: "label" }, [k.space, k.project_name].filter(Boolean).join(" · ")),
          k.excerpt && el("span", { class: "excerpt clamp" }, k.excerpt)),
        pill(k.state)))) : el("p", { class: "empty" }, t("No ticket holds this text."))),
      ks.length === 50 && el("p", { class: "meta" }, t("The 50 newest are shown: add a word.")),
    ].filter(Boolean));
  };
  let timer;
  box.addEventListener("input", () => { clearTimeout(timer); timer = setTimeout(run, 250); });
  show(
    el("div", { class: "top" }, el("div", {}, el("h1", {}, t("Search")),
      el("p", { class: "meta" }, t("In the tickets of every space you see: id, title, description and comments.")))),
    box, found);
  await run();
  box.focus();
}

/* ---------- ticket ---------- */

function describe(e) {
  const val = v => e.field === "state" ? t(STATE[v] || v) : e.field === "priority" ? t(PRIORITY[v] || v)
    : e.field.endsWith("_date") ? day(v) : v;
  if (e.field === "created") return t("created the ticket");
  if (e.field === "description") return t("changed the description");
  if (e.field === "work") return e.new ? t("logged {t}", { t: span(Number(e.new)) }) : t("removed {t} of logged time", { t: span(Number(e.old)) });
  if (e.field === "file") return e.new ? t("attached the file {name}", { name: e.new }) : t("removed the file {name}", { name: e.old });
  if (e.field === "link") return e.new ? t("added the link {url}", { url: e.new }) : t("removed the link {url}", { url: e.old });
  return t("{field}: {old} → {new}", { field: t(FIELD[e.field] || e.field), old: val(e.old) || t("empty"), new: val(e.new) || t("empty") });
}

async function ticket(id) {
  const path = "api/tickets/" + encodeURIComponent(id);
  const tk = live(path, await api(path));
  await api("api/notifications", { ticket: id });   // seen: its notices are read
  const key = id.slice(0, id.lastIndexOf("-"));
  const p = await api("api/projects/" + encodeURIComponent(key));
  setSpace(p.space);
  const canEdit = tk.role !== "reader", owner = isOwner(tk.role);
  const frozen = !canEdit || (!owner && tk.state !== "backlog");   // title and description
  // what a contributor may pick: nothing out of "Proposed", never "To do"
  const allowed = s => owner || s === tk.state || (tk.state !== "backlog" && s !== "todo");
  const f = {
    title: el("input", { value: tk.title, required: true, maxLength: 200, disabled: frozen }),
    description: el("textarea", { value: tk.description, rows: 10, maxLength: 20000, disabled: frozen,
      placeholder: t("What is needed, for whom, and how we will know it is done.") }),
    state: el("select", { disabled: !canEdit }, ME.states.map(s =>
      el("option", { value: s, selected: s === tk.state, disabled: !allowed(s) }, t(STATE[s])))),
    priority: select(ME.priorities, tk.priority, v => t(PRIORITY[v]), { disabled: !canEdit }),
    assignee: select(["", ...p.members.map(m => m.login)], tk.assignee, v => v || t("Nobody"), { disabled: !canEdit }),
    start_date: el("input", { type: "date", value: tk.start_date, disabled: !canEdit }),
    end_date: el("input", { type: "date", value: tk.end_date, disabled: !canEdit }),
    wake_date: el("input", { type: "date", value: tk.wake_date, disabled: !canEdit }),
    milestone: el("input", { value: tk.milestone, maxLength: 80, disabled: !canEdit }),
    // which agent builds it: part of what is approved, frozen like the text
    agent: el("input", { value: tk.agent, maxLength: 20, pattern: "[a-z0-9._\\-]*", autocapitalize: "none", disabled: frozen }),
  };
  f.milestone.setAttribute("list", "milestones");   // "list" is read-only as a property
  const save = async extra => {
    const body = { ...extra };
    for (const [k, c] of Object.entries(f)) if (c.value.trim() !== tk[k] && !(k in body)) body[k] = c.value;
    await api(path, body);
    f.description.sent();
    await ticket(id);
    toast(t("Saved"));
  };
  const url = el("input", { type: "url", required: true, maxLength: 2000, placeholder: t("Address (https://…)") });
  const label = el("input", { maxLength: 200, placeholder: t("Label") });
  const spent = el("input", { required: true, maxLength: 20, placeholder: t("Time (1h30, 45m)"), "aria-label": t("Time spent") });
  const when = el("input", { type: "date", value: new Date().toLocaleDateString("sv-SE"), "aria-label": t("Day") });
  const what = el("input", { maxLength: 500, placeholder: t("Note (optional)"), "aria-label": t("Note (optional)") });
  const picker = el("input", { type: "file", multiple: true, required: true, "aria-label": t("Files") });
  const weight = n => n < 1048576 ? t("{n} kB", { n: Math.max(1, Math.round(n / 1024)) }) : t("{n} MB", { n: (n / 1048576).toFixed(1) });
  const words = el("textarea", { required: true, rows: 3, maxLength: 500000, placeholder: t("Write a comment") });
  const wordsMark = draft(words, id + ":comment");
  const descMark = draft(f.description, id + ":description");   // dropped when frozen
  // a question to the person in front: what is asked, then one click to answer
  const asked = tk.state === "waiting" && tk.assignee === ME.login;
  // what whoever asked wrote after the question: it may change what is asked
  const news = asked ? tk.comments.filter(c => c.author === tk.author).pop() : null;
  // a question on a ticket this person approved: it stays with whoever asked, the answer is a comment
  const approved = tk.events.find(e => (e.field === "state" || e.field === "created") && e.new === "todo");
  const lastWord = tk.comments[tk.comments.length - 1];
  // the description reads like a comment (blocks to copy, diagrams); "Edit" opens the field
  const descField = field("Description", [f.description, descMark]);
  const descView = el("div", { class: "described" }, el("small", {}, t("Description")), rich(tk.description),
    !frozen && el("button", { type: "button", class: "quiet", onclick: () => { descView.hidden = true; descField.hidden = false; f.description.focus(); } },
      t("Edit the description")));
  descField.hidden = !!tk.description && !descMark;   // a draft of it is shown in its field
  descView.hidden = !descField.hidden;
  // what a forge attached as a document (hooks.rs), apart from the other links
  const isDoc = l => l.title.startsWith("Doc: ");
  const linkRow = l => el("li", {},
    el("a", { href: l.url, target: "_blank", rel: "noopener noreferrer" }, isDoc(l) ? l.title.slice(5) : l.title || l.url),
    canEdit && el("button", { type: "button", class: "quiet", onclick: act(async () => { await api(path + "/links", { delete: l.id }); await ticket(id); }) }, t("Remove")));
  const docs = tk.links.filter(isDoc), others = tk.links.filter(l => !isDoc(l));
  const waits = !asked && tk.state === "waiting" && approved && approved.actor === ME.login && lastWord && lastWord.author !== ME.login;
  const answer = el("textarea", { rows: 3, maxLength: 500000, placeholder: t("Your answer (optional)"), "aria-label": t("Your answer") });
  const answerMark = draft(answer, id + ":answer");
  const reply = state => act(async () => {
    if (answer.value.trim() || !state) await api(path + "/comments", { body: answer.value });   // empty and alone: refused
    answer.sent(); answer.value = "";   // sent: a later refusal must not send it twice
    if (state) await api(path, { state });
    await route();   // the ticket and the count of the menu
  });
  show(
    el("div", { class: "top" },
      el("div", {}, el("h1", {}, id + " ", pill(tk.state), " ", progress(tk)),
        el("p", { class: "meta" }, t("Created by {author} on {date}", { author: tk.author, date: stamp(tk.created) }),
          tk.parent && [" · ", t("Sub-ticket of"), " ", el("a", { href: "#/t/" + tk.parent }, tk.parent)])),
      el("a", { class: "button", href: "#/p/" + key }, p.name)),
    asked && el("div", { class: "card question" },
      el("h2", {}, t("{author} asks you", { author: tk.author })),
      rich(tk.description),
      news && el("p", { class: "note" }, t("Update of {date}", { date: stamp(news.at) })),
      news && rich(news.body),
      el("div", { class: "form" }, answer, answerMark,
        el("div", { class: "row" },
          el("button", { type: "button", class: "approve", onclick: reply("done") }, t("Done, close it")),
          owner && el("button", { type: "button", onclick: reply("todo") }, t("Answered: carry on")),
          el("button", { type: "button", class: "quiet", onclick: reply("") }, t("Only send the answer"))),
        el("small", {}, t(owner ? "Done: nothing more to do. Carry on: you approve what the ticket says comes next, with your answer."
          : "Done: nothing more to do.")))),
    tk.wake_date && el("p", { class: "note" }, t("This ticket sleeps until {date}.", { date: day(tk.wake_date) })),
    waits && el("div", { class: "card question" },
      el("h2", {}, t("{author} asks you", { author: lastWord.author })),
      rich(lastWord.body),
      el("div", { class: "form" }, answer, answerMark,
        el("div", { class: "row" }, el("button", { type: "button", class: "approve", onclick: reply("") }, t("Send the answer"))),
        el("small", {}, t("Your answer is a comment: whoever asked takes the ticket again.")))),
    tk.state === "backlog" && el("p", { class: "note" }, owner
      ? t("Proposal: approve it to mark it as ready to be done.")
      : t("This ticket waits for the approval of a project owner.")),
    tk.state !== "backlog" && canEdit && !owner && el("p", { class: "note" },
      t("Approved: only a project owner can change its title and description.")),
    el("form", { class: "card form", onsubmit: act(() => save()) },
      field("Title", f.title),
      descView,
      descField,
      el("div", { class: "fields" },
        field("State", f.state), field("Priority", f.priority), field("Assignee", f.assignee),
        field("Start", f.start_date), field("End", f.end_date),
        field("Wake up", f.wake_date, "Until that day the ticket is out of « For me » and no agent takes it."),
        field("Milestone", f.milestone, "A family of tickets: a phase, a version."),
        field("Agent", f.agent, "The agent role that builds it (dev, doc); empty: the project lead; routine: no agent takes it, a session or the night routine does.")),
      el("datalist", { id: "milestones" }, [...new Set(p.tickets.map(k => k.milestone).filter(Boolean))].sort().map(s => el("option", { value: s }))),
      canEdit && el("div", { class: "row" },
        el("button", { class: "primary" }, t("Save")),
        owner && tk.state === "backlog" && el("button", { type: "button", class: "approve", onclick: act(() => save({ state: "todo" })) }, t("Approve")))),
    tk.children.length > 0 && el("h2", {}, t("Sub-tickets"), " ", progress(tk)),
    tk.children.length > 0 && el("div", { class: "card" }, el("ul", { class: "list" }, tk.children.map(k => el("li", {},
      el("a", { href: "#/t/" + k.id }, el("strong", {}, k.id + " · " + k.title)),
      el("span", { class: "kmeta" }, k.agent && el("span", { class: "pill agent" }, k.agent),
        k.assignee && el("span", { class: "label" }, person(k.assignee)), pill(k.state)))))),
    docs.length > 0 && el("h2", {}, t("Documentation")),
    docs.length > 0 && el("div", { class: "card" }, el("ul", { class: "list" }, docs.map(linkRow))),
    el("h2", {}, t("Links")),
    el("div", { class: "card" },
      others.length ? el("ul", { class: "list" }, others.map(linkRow))
        : el("p", { class: "empty" }, t("No link yet.")),
      canEdit && el("form", { class: "row", onsubmit: act(async () => { await api(path + "/links", { url: url.value, title: label.value }); await ticket(id); }) },
        url, label, el("button", {}, t("Add the link")))),
    el("h2", {}, t("Files")),
    el("div", { class: "card" },
      tk.files.length ? el("ul", { class: "list" }, tk.files.map(f => el("li", {},
        el("span", {}, el("a", { href: path + "/files/" + f.id, target: "_blank", rel: "noopener noreferrer" }, f.name),
          el("span", { class: "label" }, " · " + weight(f.size) + " · " + f.author + " · " + stamp(f.at))),
        canEdit && el("button", { type: "button", class: "quiet", onclick: act(async () => { await api(path + "/files", { delete: f.id }); await ticket(id); }) }, t("Remove")))))
        : el("p", { class: "empty" }, t("No file yet.")),
      canEdit && el("form", { class: "row", onsubmit: act(async () => {
        try { for (const f of picker.files) await upload(path + "/files?name=" + encodeURIComponent(f.name), f); }
        finally { await ticket(id); }   // what was sent before a refusal is shown
      }) },
        picker, el("button", {}, t("Attach")), el("small", {}, t("Up to {n} MB per file.", { n: ME.max_file_mb })))),
    el("h2", {}, t("Time")),
    el("div", { class: "card" },
      el("p", { class: "times" },
        el("strong", {}, t("Time spent: {t}", { t: span(tk.time.logged) })),
        ["todo", "in_progress", "waiting"].filter(s => tk.time.states[s] > 0).map(s =>
          el("span", { class: "label" }, t("{label}: {value}", { label: t(STATE[s]), value: clock(tk.time.states[s]) })))),
      tk.work.length > 0 && el("ul", { class: "list" }, tk.work.map(w => el("li", {},
        el("span", {}, el("strong", {}, span(w.minutes)),
          el("span", { class: "label" }, " · " + w.login + " · " + new Date(w.at * 1000).toLocaleDateString(LOCALE(), { dateStyle: "medium" })),
          w.note && " · " + w.note),
        canEdit && (owner || w.login === ME.login) && el("button", { type: "button", class: "quiet",
          onclick: act(async () => { await api(path + "/work", { delete: w.id }); await ticket(id); }) }, t("Remove"))))),
      canEdit && el("form", { class: "row", onsubmit: act(async () => {
        const m = minutes(spent.value);
        if (!m) throw new Error(t("invalid time"));
        await api(path + "/work", { minutes: m, day: when.value, note: what.value });
        await ticket(id);
      }) },
        spent, when, what, el("button", {}, t("Log time")))),
    el("h2", {}, t("Comments")),
    el("div", { class: "card" },
      tk.comments.length ? tk.comments.map(c => el("div", { class: "comment" },
        el("p", { class: "meta" }, person(c.author), " · " + stamp(c.at)), rich(c.body)))
        : el("p", { class: "empty" }, t("No comment yet.")),
      el("form", { class: "form", onsubmit: act(async () => { await api(path + "/comments", { body: words.value }); words.sent(); words.value = ""; await ticket(id); }) },
        words, wordsMark, el("div", { class: "row" }, el("button", {}, t("Send"))))),
    el("h2", {}, t("History")),
    el("div", { class: "card" }, el("ul", { class: "list history" }, tk.events.map(e => el("li", {},
      el("span", {}, person(e.actor), " " + describe(e)), el("span", { class: "label" }, stamp(e.at)))))),
  );
}

/* ---------- notifications ---------- */

const NOTICE = { comment: "{actor} commented", state: "{actor} moved the ticket to {state}", assigned: "{actor} assigned the ticket to you",
  wake: "This ticket is back: its wake-up date has come" };
const said = n => t(NOTICE[n.kind] || n.kind, { actor: n.actor, state: t(STATE[n.detail] || n.detail) });

// The entry of the menu, with the number of unread notices, kept up to date
// while the page stays open. A plain request, not api(): a lost session must
// not reload the page from under someone who is typing. On a failure the
// entry is left as it is.
let UNREAD = 0;
async function bell() {
  const link = $("#bell");
  const here = location.hash.startsWith("#/n");
  if (here) link.setAttribute("aria-current", "page"); else link.removeAttribute("aria-current");
  if (location.hash.startsWith("#/s")) $("#settings").setAttribute("aria-current", "page"); else $("#settings").removeAttribute("aria-current");
  if (location.hash.startsWith("#/u")) $("#me").setAttribute("aria-current", "page"); else $("#me").removeAttribute("aria-current");
  let n;
  try {
    const r = await fetch("api/notifications", { redirect: "manual" });
    if (!signedIn(r) || !r.ok) return;
    n = await r.json();
  } catch (e) { return; }
  link.replaceChildren(...[t("Notifications"), n.unread > 0 && el("span", { class: "pill count" }, String(n.unread))].filter(Boolean));
  const more = n.unread > UNREAD;
  UNREAD = n.unread;
  if (more && here) notices().catch(() => {});   // the list in front follows
}
// Only while the tab is in front: a tab left open in the background must not
// keep the session at the proxy alive. Back in front: at once, then every 30 s.
setInterval(() => { if (!document.hidden) { bell(); follow(); } }, 30000);
document.addEventListener("visibilitychange", () => { if (!document.hidden) { bell(); follow(); } });

// A ticket or a project in front follows what others change (a state, a
// comment, an agent's link) without F5. Its data is fetched again on the same
// beat as the notices and the page is drawn again only if the data changed
// and nobody is typing in it: no field in focus, every field as it was drawn.
// ponytail: whole-view polling every 30 s, push (SSE) if that is too slow.
let LIVE = null;
const fields = () => JSON.stringify([...$("#view").querySelectorAll("input, textarea, select")].map(i => i.type === "checkbox" ? i.checked : i.value));
// The time spent in each state grows by itself: not a change worth a redraw.
const sign = d => JSON.stringify({ ...d, time: d.time && d.time.logged });
function live(path, data) {
  LIVE = { path, hash: location.hash, sig: sign(data) };
  return data;
}
async function follow() {
  const l = LIVE;
  if (!l || l.hash !== location.hash || !l.form) return;
  let d;
  try {
    const r = await fetch(l.path, { redirect: "manual" });   // like bell(): a lost session must not reload the page
    if (!signedIn(r) || !r.ok) return;
    d = await r.json();
  } catch (e) { return; }
  const busy = $("#view").contains(document.activeElement) && document.activeElement.matches("input, textarea, select");
  if (LIVE !== l || l.hash !== location.hash || sign(d) === l.sig || busy || fields() !== l.form) return;
  const y = scrollY;
  await route();
  scrollTo(0, y);
}

async function notices() {
  const n = await api("api/notifications");
  // one line per ticket: its newest notice, and how many came before (opening the ticket reads them all)
  const groups = [];
  for (const i of n.items) {
    const g = groups.find(x => x.ticket === i.ticket);
    if (g) { g.more++; g.read = g.read && i.read; } else groups.push({ ...i, more: 0 });
  }
  const mail = el("input", { type: "checkbox", checked: !!ME.mail, onchange: act(async ev => {
    await api("api/me", { mail: ev.target.checked });
    ME.mail = ev.target.checked;
    toast(t("Saved"));
  }) });
  const topic = el("input", { value: ME.ntfy_topic || "", maxLength: 64, pattern: "[A-Za-z0-9_\\-]*", placeholder: (ME.ntfy_prefix || "") + ME.login,
    "aria-label": t("ntfy topic") });
  show(
    el("div", { class: "top" },
      el("div", {}, el("h1", {}, t("Notifications")),
        el("p", { class: "meta" }, t("What others do on the tickets you created, are assigned to or commented."))),
      n.unread > 0 && el("button", { type: "button", onclick: act(async () => { await api("api/notifications", { read: "all" }); await notices(); await bell(); }) },
        t("Mark all as read"))),
    el("div", { class: "card" }, groups.length ? el("ul", { class: "list notices" }, groups.map(i => el("li", { class: i.read ? "" : "unread" },
      el("a", { href: "#/t/" + i.ticket },
        el("strong", {}, i.ticket + " · " + i.title),
        el("span", {}, said(i)),
        i.kind === "comment" && el("span", { class: "excerpt clamp" }, i.detail),
        i.more > 0 && el("span", { class: "label" }, tp(i.more, "and {n} earlier notice", "and {n} earlier notices"))),
      el("span", { class: "label" }, stamp(i.at)))))
      : el("p", { class: "empty" }, t("Nothing yet."))),
    el("div", { class: "card" }, el("label", { class: "inline" }, mail,
      ME.email ? t("Also send them to me by e-mail ({email})", { email: ME.email }) : t("Also send them to me by e-mail"))),
    ME.ntfy && el("form", { class: "card form", onsubmit: act(async () => {
      await api("api/me", { ntfy_topic: topic.value });
      ME.ntfy_topic = topic.value;
      toast(t("Saved"));
    }) },
      el("h2", {}, t("On my phone (ntfy)")),
      el("div", { class: "row" }, topic, el("button", {}, t("Save"))),
      el("small", {}, ME.ntfy_prefix
        ? t("Choose a topic that starts with {prefix} and subscribe to it in the ntfy app. Empty: no push.", { prefix: ME.ntfy_prefix })
        : t("Choose a topic and subscribe to it in the ntfy app. Empty: no push."))),
  );
}

/* ---------- profile ---------- */

// Framing a photo before it is sent: the image under a fixed square, with the disc (or the
// square of an agent) it will show in. Dragging moves it, the arrow keys too; the slider zooms.
// Answers that square, 256 pixels wide, as a JPEG; null when the person gives up. Drawn from
// the file itself: the page loads no image from an address of its own making.
async function framed(file, bot) {
  let img;
  try { img = await createImageBitmap(file, { imageOrientation: "from-image" }); } catch (e) { throw new Error(t("not an image (PNG, JPEG or WebP)")); }
  const N = 256, base = Math.min(img.width, img.height);
  let zoom = 1, cx = img.width / 2, cy = img.height / 2, last = null;
  const view = el("canvas", { width: N, height: N, tabIndex: 0, "aria-label": t("Photo: drag it or use the arrow keys to place it") });
  const draw = () => {
    const side = base / zoom, g = view.getContext("2d");
    cx = Math.min(img.width - side / 2, Math.max(side / 2, cx));   // the square never leaves the image
    cy = Math.min(img.height - side / 2, Math.max(side / 2, cy));
    g.fillStyle = "#fff";   // a JPEG has no transparency
    g.fillRect(0, 0, N, N);
    g.imageSmoothingQuality = "high";
    g.drawImage(img, cx - side / 2, cy - side / 2, side, side, 0, 0, N, N);
  };
  // the image follows the pointer: screen pixels, turned into pixels of the image
  const move = (dx, dy) => { const k = base / zoom / (view.clientWidth || N); cx -= dx * k; cy -= dy * k; draw(); };
  view.addEventListener("pointerdown", ev => { last = ev; view.setPointerCapture(ev.pointerId); });
  view.addEventListener("pointermove", ev => { if (last) { move(ev.clientX - last.clientX, ev.clientY - last.clientY); last = ev; } });
  for (const end of ["pointerup", "pointercancel"]) view.addEventListener(end, () => { last = null; });
  view.addEventListener("keydown", ev => {
    const step = { ArrowLeft: [-16, 0], ArrowRight: [16, 0], ArrowUp: [0, -16], ArrowDown: [0, 16] }[ev.key];
    if (step) { ev.preventDefault(); move(...step); }
  });
  const slider = el("input", { type: "range", min: 1, max: 4, step: 0.01, value: 1, oninput: ev => { zoom = Number(ev.target.value); draw(); } });
  draw();
  return new Promise(done => {
    const box = el("dialog", { class: "cropper", onclose: () => { box.remove(); done(null); } },
      el("h2", {}, t("Frame the photo")),
      el("p", { class: "meta" }, t("Drag the photo to place it. The slider zooms in.")),
      el("div", { class: "frame" + (bot ? " bot" : "") }, view),
      el("label", { class: "inline" }, t("Zoom"), slider),
      el("div", { class: "row" },
        el("button", { type: "button", class: "primary", onclick: () => view.toBlob(blob => { done(blob); box.close(); }, "image/jpeg", 0.9) }, t("Save")),
        el("button", { type: "button", onclick: () => box.close() }, t("Cancel"))));
    document.body.append(box);
    box.showModal();
  });
}

// The photo of one account: choose it (framed, then sent) or remove it. choose: false for
// the account of someone else, whose photo an administrator can only remove.
function photoButtons(login, choose = true) {
  const path = "api/avatars/" + encodeURIComponent(login);
  const changed = async version => {
    if (version) ME.photos[login] = version; else delete ME.photos[login];
    frame();
    await profile();
    toast(t("Saved"));
  };
  const pick = el("input", { type: "file", accept: "image/png,image/jpeg,image/webp", hidden: true, onchange: act(async ev => {
    const file = ev.target.files[0];
    ev.target.value = "";   // the same file can be chosen again
    const blob = file && await framed(file, (ME.agents || []).includes(login));
    if (blob) await changed((await upload(path, blob)).photo);
  }) });
  return el("div", { class: "row" }, choose && pick,
    // one main button on the page: mine, while I have no photo
    choose && el("button", { type: "button", class: login === ME.login && !ME.photos[login] ? "primary" : "", onclick: () => pick.click() },
      t(ME.photos[login] ? "Change the photo" : "Choose a photo")),
    ME.photos[login] && el("button", { type: "button", onclick: act(async () => changed((await api(path, { delete: true })).photo)) }, t("Remove the photo")));
}

// One idea: my photo. An administrator also sets the photo of the agents, which never open
// the page, and can remove the photo of someone (not choose it).
// What an hour of my time costs: given here, seen by me, the administrators and the owners of the projects.
function myRate() {
  const m = ME.money, cents = el("input", { type: "number", min: 0, max: 10000, step: "0.5", value: m.rate ? m.rate / 100 : "", placeholder: "0", "aria-label": t("Hourly rate") });
  const code = select(m.currencies.includes(m.rate_currency) ? m.currencies : [m.rate_currency, ...m.currencies], m.rate_currency, c => c);
  return el("form", { class: "card form", onsubmit: act(async () => {
    const rate = Math.round(Number(cents.value || 0) * 100);
    await api("api/rates", { rate, rate_currency: code.value });
    Object.assign(ME.money, { rate, rate_currency: code.value });
    toast(t("Saved"));
  }) },
    el("h2", {}, t("My hourly rate")),
    el("p", { class: "meta" }, t("What an hour of your time costs. The Time page multiplies it by the time you log. Only you, the administrators and the owners of your projects see it.")),
    el("div", { class: "row" }, cents, code, el("button", { class: "primary" }, t("Save"))));
}

async function profile() {
  const me = await api("api/me");   // what others changed since the page was opened
  ME.photos = me.photos || {};
  ME.money = me.money;
  const agents = ME.admin ? [...new Set(ME.agents || [])].sort() : [];
  const people = ME.admin ? Object.keys(ME.photos).filter(l => l !== ME.login && !agents.includes(l)).sort() : [];
  const rows = (logins, choose) => el("ul", { class: "list" },
    logins.map(l => el("li", {}, el("span", { class: "person" }, avatar(l, "lg"), l), photoButtons(l, choose))));
  show(
    el("div", { class: "top" }, el("div", {}, el("h1", {}, t("My profile")),
      el("p", { class: "meta" }, t("Your photo shows next to your name: tickets, comments, history, Time page.")))),
    el("div", { class: "card profile" }, avatar(ME.login, "xl"),
      el("div", {}, el("strong", {}, ME.name || ME.login), el("p", { class: "meta" }, ME.login), photoButtons(ME.login),
        el("small", {}, t("PNG, JPEG or WebP. You frame the photo before it is saved.")))),
    myRate(),
    agents.length > 0 && el("div", { class: "card" }, el("h2", {}, t("Agents")),
      el("p", { class: "meta" }, t("An agent never opens this page: an administrator chooses its photo. It stays a square.")),
      rows(agents, true)),
    people.length > 0 && el("div", { class: "card" }, el("h2", {}, t("People")),
      el("p", { class: "meta" }, t("An administrator can remove the photo of someone. Their initials come back.")),
      rows(people, false)),
  );
}

/* ---------- settings (administrators) ---------- */

// The currency amounts are shown in, and what one dollar is worth in each currency (agents cost dollars).
function currencies() {
  const m = ME.money, code = select(m.currencies, m.currency, c => c);
  const table = el("textarea", { rows: 8, spellcheck: false, "aria-label": t("Value of one dollar") });
  table.value = m.currencies.map(c => c + " " + m.fx[c]).join("\n");
  const prices = el("textarea", { rows: 6, spellcheck: false, "aria-label": t("Price of a million tokens") });
  prices.value = Object.entries(m.prices).map(([model, p]) => model + " " + p.join(" ")).join("\n");
  return el("form", { class: "card form", onsubmit: act(async () => {
    const fx = {};
    for (const l of table.value.split("\n").map(x => x.trim()).filter(Boolean)) {
      const [c, n] = l.split(/\s+/);
      fx[c.toUpperCase()] = Number((n || "").replace(",", "."));
    }
    const priceTable = {};
    for (const l of prices.value.split("\n").map(x => x.trim()).filter(Boolean)) {
      const [model, ...p] = l.split(/\s+/);
      priceTable[model] = p.map(n => Number(n.replace(",", ".")));
    }
    await api("api/rates", { fx, currency: code.value, prices: priceTable });
    ME.money = (await api("api/me")).money;
    await settings();
    toast(t("Saved"));
  }) },
    el("h2", {}, t("Currencies")),
    el("p", { class: "meta" }, t("Amounts are shown in this currency. Each member gives their hourly rate in their own currency; agents cost dollars.")),
    field("Shown in", code),
    field("Value of one dollar", table, "One currency per line: its code (EUR) and what one dollar is worth in it (0.92). Change the values when the rates move."),
    el("p", { class: "meta" }, m.fx_date ? t("Rates entered on {date}.", { date: day(m.fx_date) }) : t("Rates not entered yet: these are indicative values.")),
    field("Price of a million tokens", prices, "One model per line: its name, then the dollars for a million tokens of input, output, cache read and cache write (1 hour). A run that reports no cost is estimated with it."),
    el("p", { class: "meta" }, t("Prices dated {date}, read at {source}.", { date: day(m.prices_date), source: m.prices_source })),
    el("div", { class: "row" }, el("button", { class: "primary" }, t("Save"))));
}

async function settings() {
  const s = await api("api/settings");
  const secret = set => el("input", { type: "password", maxLength: 300, autocomplete: "new-password", placeholder: set ? t("Unchanged") : "" });
  const f = {
    public_url: el("input", { type: "url", value: s.public_url, maxLength: 300, placeholder: "https://plan.example.org" }),
    smtp_host: el("input", { value: s.smtp_host, maxLength: 200, placeholder: "smtp.example.org" }),
    smtp_port: el("input", { type: "number", min: 1, max: 65535, value: s.smtp_port }),
    smtp_user: el("input", { value: s.smtp_user, maxLength: 200, autocomplete: "off" }),
    smtp_password: secret(s.smtp_password_set),
    mail_from: el("input", { value: s.mail_from, maxLength: 200, placeholder: "Cairn Plan <plan@example.org>" }),
    ntfy_url: el("input", { type: "url", value: s.ntfy_url, maxLength: 300, placeholder: "https://ntfy.example.org" }),
    ntfy_token: secret(s.ntfy_token_set),
    ntfy_prefix: el("input", { value: s.ntfy_prefix, maxLength: 32, pattern: "[A-Za-z0-9_\\-]*", placeholder: "plan-" }),
    agent_hook_url: el("input", { type: "url", value: s.agent_hook_url, maxLength: 300, placeholder: "http://127.0.0.1:9095/hook" }),
    agent_hook_secret: secret(s.agent_hook_secret_set),
  };
  const save = act(async () => {
    const body = {};
    for (const [k, c] of Object.entries(f)) body[k] = c.value;
    await api("api/settings", body);
    await settings();
    toast(t("Saved"));
  });
  const test = kind => el("button", { type: "button", onclick: act(async () => {
    await api("api/settings/test", { kind });
    toast(t(kind === "hook" ? "The agent answered." : "Sent to you. Check that it arrives."));
  }) }, t("Send a test"));
  show(
    el("div", { class: "top" }, el("div", {}, el("h1", {}, t("Settings")),
      el("p", { class: "meta" }, t("How notifications leave the page. What is saved here wins over the configuration file.")))),
    el("form", { class: "card form", onsubmit: save },
      el("h2", {}, t("Address")),
      field("Address of this page", f.public_url, "Used for the link in a notification."),
      el("h2", {}, t("E-mail")),
      el("div", { class: "fields" },
        field("SMTP server", f.smtp_host, "Empty: no e-mail is sent."),
        field("Port", f.smtp_port, "465: TLS from the start. Any other: STARTTLS."),
        field("User", f.smtp_user),
        field("Password", f.smtp_password, "Leave empty to keep the one that is saved."),
        field("Sender", f.mail_from, "An address your SMTP server accepts.")),
      el("h2", {}, t("Phone (ntfy)")),
      el("div", { class: "fields" },
        field("ntfy server", f.ntfy_url, "Empty: no push is sent."),
        field("Access token", f.ntfy_token, "Leave empty to keep the one that is saved."),
        field("Topic prefix", f.ntfy_prefix, "Each member chooses a topic that starts with it. Give the token write access to these topics only.")),
      el("h2", {}, t("Agent")),
      el("p", {}, el("a", { class: "button", href: "#/agent" }, t("What the agent is doing"))),
      el("p", { class: "meta" }, t("Called as soon as a ticket is approved, and when whoever approved a waiting ticket answers it: an agent can then take the ticket at once.")),
      el("div", { class: "fields" },
        field("Webhook address", f.agent_hook_url, "Empty: nothing is called."),
        field("Secret", f.agent_hook_secret, "16 characters or more, the same as in the agent. It signs each call. Leave empty to keep the one that is saved.")),
      el("div", { class: "row" }, el("button", { class: "primary" }, t("Save")))),
    currencies(s),
    el("div", { class: "card form" },
      el("h2", {}, t("Try it")),
      el("p", { class: "meta" }, t("Sends one message to you with the settings that are saved. The answer of the server is shown if it fails.")),
      el("div", { class: "row" }, el("span", {}, t("E-mail")), test("mail"), el("span", {}, t("Phone (ntfy)")), test("ntfy"), el("span", {}, t("Agent")), test("hook"))),
  );
}

/* ---------- time ---------- */

// The time of a project (#/p/KEY/time) or of a space (#/e/<space>): one
// sentence (hours logged, what people and agents cost, tickets done), then
// one view. The server sends the tickets and the entries; the page adds up.
const TIME = { view: "summary", period: "30" };
try { Object.assign(TIME, JSON.parse(localStorage.getItem("cairn-plan-time2") || "{}")); } catch (e) { /* storage blocked */ }
const saveTime = () => { try { localStorage.setItem("cairn-plan-time2", JSON.stringify(TIME)); } catch (e) { /* storage blocked */ } };
const PERIODS = { "7": "7 days", "30": "30 days", "90": "90 days", all: "All" };
const VIEWS = { summary: "Summary", sheet: "Timesheet", journal: "Journal" };
const DAY = 86400;
const SHOWN = {};   // lists the reader asked to see in full

function svg(tag, attrs, ...kids) {
  const e = document.createElementNS("http://www.w3.org/2000/svg", tag);
  for (const [k, v] of Object.entries(attrs || {})) e.setAttribute(k, v);
  for (const k of kids.flat(Infinity)) if (k != null && k !== false) e.append(k);
  return e;
}
const sum = (xs, f) => xs.reduce((a, x) => a + f(x), 0);
const hours = min => min ? span(min) : "–";
const startOf = { day: sec => { const d = new Date(sec * 1000); d.setHours(0, 0, 0, 0); return d.getTime() / 1000; },
  week: sec => { const d = new Date(sec * 1000); d.setHours(0, 0, 0, 0); d.setDate(d.getDate() - (d.getDay() + 6) % 7); return d.getTime() / 1000; },
  month: sec => { const d = new Date(sec * 1000); return new Date(d.getFullYear(), d.getMonth(), 1).getTime() / 1000; } };
const nextOf = { day: s => startOf.day(s + 30 * 3600), week: s => startOf.week(s + 7.5 * DAY), month: s => startOf.month(s + 32 * DAY) };
const slotLabel = { day: s => new Date(s * 1000).toLocaleDateString(LOCALE(), { weekday: "short", day: "numeric" }),
  week: s => new Date(s * 1000).toLocaleDateString(LOCALE(), { day: "numeric", month: "short" }),
  month: s => new Date(s * 1000).toLocaleDateString(LOCALE(), { month: "short", year: "2-digit" }) };

/** The figures of a report for the period chosen. */
function timeFigures(r) {
  const from = TIME.period === "all" ? 0 : startOf.day(r.now - (Number(TIME.period) - 1) * DAY);
  const agent = login => r.agents.includes(login);
  const work = r.work.filter(w => w.at >= from);
  const costs = r.costs.filter(k => k.at >= from);
  const byId = Object.fromEntries(r.tickets.map(k => [k.id, k]));
  const closed = r.tickets.filter(k => k.state === "done" && k.closed >= from);
  // what an entry of someone costs, in cents; nothing when the rates are not shown to the viewer
  const cents = w => r.money && !agent(w.login) ? w.minutes * (r.rates[w.login] || 0) / 60 : 0;
  const unit = TIME.period === "7" ? "day" : TIME.period === "all" ? "month" : "week";
  const first = work.length ? Math.min(...work.map(w => w.at)) : r.now;
  const slots = [];
  for (let s = startOf[unit](TIME.period === "all" ? first : from); s <= r.now; s = nextOf[unit](s)) slots.push(s);
  const slotOf = at => slots.indexOf(startOf[unit](at));
  return { from, agent, work, costs, byId, closed, cents, unit, slots: slots.slice(-24), slotOf: at => slotOf(at) - Math.max(0, slots.length - 24),
    minutes: sum(work, w => w.minutes), people: sum(work.filter(w => !agent(w.login)), w => w.minutes),
    agents: sum(work.filter(w => agent(w.login)), w => w.minutes), money: sum(work, cents), usd: sum(costs, k => k.usd),
    lead: closed.length ? sum(closed, k => k.closed - k.created) / closed.length : 0 };
}
// An amount in an ISO currency ("EUR"): the browser knows the sign and where it goes.
const fmt = (n, code) => new Intl.NumberFormat(LOCALE(), { style: "currency", currency: code, minimumFractionDigits: Math.abs(n) < 100 ? 2 : 0, maximumFractionDigits: Math.abs(n) < 100 ? 2 : 0 }).format(n);
const amount = (r, cents) => fmt(cents / 100, r.currency);
// Dollars (what agents cost) in the currency of the page, through the table of the settings.
const money = usd => fmt(usd * (ME.money.fx[ME.money.currency] || 1), ME.money.currency);
const dollars = usd => usd.toLocaleString(LOCALE(), { minimumFractionDigits: 2, maximumFractionDigits: 2 }) + " $";
/** What people and agents cost, as one amount in the currency of the page: "1 196 €", or nothing. `by`: cents per
 *  currency as each person gave their rate (dollars for agents): it shows under the pointer, "800 CHF + 300 € + 9,04 $". */
const cost = (r, cents, usd, by) => {
  const all = cents + usd * r.per_usd * 100;
  if (all < 50) return "";
  const parts = Object.entries(by || {}).filter(([, v]) => v >= 1).map(([code, v]) => fmt(v / 100, code));
  return el("span", { title: parts.length > 1 || (by && Object.keys(by).some(c => c !== r.currency)) ? parts.join(" + ") : "" }, amount(r, all));
};
const addBy = (by, code, cents) => { by[code] = (by[code] || 0) + cents; };
/** What one entry of time adds to a detail by currency: the rate of the person, in the currency they gave it. */
const peopleBy = (r, f, w, by) => { const o = r.rates_own[w.login]; if (r.money && o && !f.agent(w.login)) addBy(by, o.currency, w.minutes * o.rate / 60); };
const mergeBy = rows => { const by = {}; for (const x of rows) for (const [code, v] of Object.entries(x.by)) addBy(by, code, v); return by; };

/** Rows {key, label, min, cents, usd}, largest first, from the entries and the agent costs grouped by a key. */
function timeRows(r, f, keyOf, label) {
  const rows = new Map();
  const row = k => rows.get(k) || rows.set(k, { key: k, label: label(k), min: 0, cents: 0, usd: 0, by: {}, slots: f.slots.map(() => 0) }).get(k);
  for (const w of f.work) {
    const x = row(keyOf(w)); x.min += w.minutes; x.cents += f.cents(w); peopleBy(r, f, w, x.by);
    const i = f.slotOf(w.at); if (i >= 0) x.slots[i] += w.minutes;
  }
  for (const k of f.costs) { const x = row(keyOf(k)); x.usd += k.usd; addBy(x.by, "USD", k.usd * 100); }
  return [...rows.values()].sort((a, b) => b.min - a.min || b.usd - a.usd);
}
const byWho = (r, f) => timeRows(r, f, w => w.login, login => person(login));
const byPlace = (r, f) => r.scope === "space"
  ? timeRows(r, f, w => f.byId[w.ticket].project, key => el("a", { href: "#/p/" + key + "/time" }, (r.projects.find(p => p.key === key) || {}).name || key))
  : timeRows(r, f, w => w.ticket, id => el("span", {}, el("a", { href: "#/t/" + id }, id), " ", f.byId[id].title));

/** The first rows of a long list, and a button to see them all. */
function limited(name, rows, n, draw) {
  const all = SHOWN[name] || rows.length <= n + 1;
  return [draw(all ? rows : rows.slice(0, n)),
    !all && el("button", { type: "button", class: "quiet more", onclick: () => { SHOWN[name] = true; redraw(); } }, t("Show all ({n})", { n: rows.length }))];
}

function timeHero(r, f) {
  const period = TIME.period === "all" ? t("logged since the start") : t("logged over the last {n} days", { n: TIME.period });
  const fact = (label, value, extra) => el("span", { class: "fact" }, el("small", {}, t(label)), el("b", {}, value), extra && el("span", {}, extra));
  return el("div", { class: "time-hero" },
    el("p", { class: "big" }, el("strong", {}, f.minutes ? span(f.minutes) : t("No time")), " ", el("span", {}, period)),
    el("div", { class: "facts" },
      fact("People", hours(f.people), r.money && f.money >= 50 ? amount(r, f.money) : ""),
      fact("Agents", hours(f.agents), f.usd ? amount(r, f.usd * r.per_usd * 100) : ""),
      fact("Tickets done", String(f.closed.length), f.closed.length ? t("in {t} on average", { t: clock(f.lead) }) : "")));
}

/* Summary: bars over time (people, agents), then who and where */
function timeSummary(r, f) {
  const stacks = f.slots.map(() => [0, 0]);
  for (const w of f.work) { const i = f.slotOf(w.at); if (i >= 0) stacks[i][f.agent(w.login) ? 1 : 0] += w.minutes; }
  const top = Math.max(60, ...stacks.map(([a, b]) => a + b)), n = f.slots.length;
  const W = 760, H = 190, B = 22, T = 18, bw = Math.min(56, (W / n) * 0.62);
  const x = i => (i + 0.5) * W / n, y = v => T + (H - B - T) * (1 - v / top);
  const chart = svg("svg", { viewBox: `0 0 ${W} ${H}`, class: "chart", role: "img", "aria-label": t("Time logged over the period") },
    svg("line", { x1: 0, x2: W, y1: y(0), y2: y(0), class: "grid-line" }),
    f.slots.map((s, i) => {
      const [p, a] = stacks[i];
      return [
        p > 0 && svg("rect", { x: x(i) - bw / 2, y: y(p), width: bw, height: y(0) - y(p), rx: 3, class: "c1" },
          svg("title", {}, `${slotLabel[f.unit](s)} · ${t("People")} : ${span(p)}`)),
        a > 0 && svg("rect", { x: x(i) - bw / 2, y: y(p + a), width: bw, height: Math.max(1, y(p) - y(p + a) - (p ? 2 : 0)), rx: 3, class: "c2" },
          svg("title", {}, `${slotLabel[f.unit](s)} · ${t("Agents")} : ${span(a)}`)),
        n <= 14 && p + a > 0 && svg("text", { x: x(i), y: y(p + a) - 5, class: "axis value", "text-anchor": "middle" }, span(p + a)),
        (n <= 14 || i % Math.ceil(n / 12) === 0) && svg("text", { x: x(i), y: H - 5, class: "axis", "text-anchor": "middle" }, slotLabel[f.unit](s))];
    }));
  const list = (title, name, rows, n) => el("div", { class: "card" }, el("h2", {}, t(title)),
    rows.length ? limited(name, rows, n, shown => el("div", { class: "bars" }, shown.map(x => el("div", { class: "hbar" },
      el("span", { class: "name" }, x.label),
      el("span", { class: "track" }, el("span", { class: "fill", style: "width:" + Math.max(x.min ? 1 : 0, Math.round(100 * x.min / (rows[0].min || 1))) + "%" })),
      el("span", { class: "value" }, hours(x.min)),
      el("small", {}, cost(r, x.cents, x.usd, x.by))))))
      : el("p", { class: "empty" }, t("No time in this period.")));
  return [
    el("div", { class: "card" },
      el("div", { class: "row" }, el("h2", {}, t({ day: "Per day", week: "Per week", month: "Per month" }[f.unit])),
        el("span", { class: "legend" }, el("span", {}, el("i", { class: "sw c1" }), t("People")), el("span", {}, el("i", { class: "sw c2" }), t("Agents")))),
      f.work.length ? chart : el("p", { class: "empty" }, t("No time in this period."))),
    el("div", { class: "grid two" },
      list("Who", "who", byWho(r, f), 8),
      list(r.scope === "space" ? "Which project" : "Which ticket", "place", byPlace(r, f), 8))];
}

/* Timesheet: one row per person, one column per day, week or month; then the same per project or ticket */
function timeSheet(r, f) {
  const sheet = (head, name, rows, n) => el("div", { class: "card table-wrap" }, rows.length ? limited(name, rows, n, shown => el("table", { class: "sheet" },
    el("thead", {}, el("tr", {}, el("th", {}, t(head)), f.slots.map(s => el("th", { class: "num" }, slotLabel[f.unit](s))),
      el("th", { class: "num" }, t("Total")), el("th", { class: "num" }, t("Cost")))),
    el("tbody", {}, shown.map(x => el("tr", {}, el("td", {}, x.label),
      x.slots.map(m => el("td", { class: "num" + (m ? "" : " none") }, m ? span(m) : "·")),
      el("td", { class: "num total" }, hours(x.min)), el("td", { class: "num" }, cost(r, x.cents, x.usd, x.by) || "–")))),
    el("tfoot", {}, el("tr", {}, el("td", {}, t("Total")),
      f.slots.map((s, i) => el("td", { class: "num" }, hours(sum(rows, x => x.slots[i])))),
      el("td", { class: "num total" }, hours(sum(rows, x => x.min))), el("td", { class: "num" }, cost(r, sum(rows, x => x.cents), sum(rows, x => x.usd), mergeBy(rows)) || "–")))))
    : el("p", { class: "empty" }, t("No time in this period.")));
  return [sheet("Person", "who", byWho(r, f), 12), sheet(r.scope === "space" ? "Project" : "Ticket", "place", byPlace(r, f), 10)];
}

/* Journal: the entries day by day, newest first */
function timeJournal(r, f) {
  const days = new Map();
  for (const w of [...f.work].sort((a, b) => b.at - a.at)) {
    const d = startOf.day(w.at);
    days.set(d, [...(days.get(d) || []), w]);
  }
  const rows = [...days.entries()];
  if (!rows.length) return el("div", { class: "card" }, el("p", { class: "empty" }, t("No time in this period.")));
  return limited("journal", rows, 10, shown => el("div", { class: "journal" }, shown.map(([d, ws]) => el("section", { class: "card" },
    el("h2", {}, el("span", {}, new Date(d * 1000).toLocaleDateString(LOCALE(), { weekday: "long", day: "numeric", month: "long" })),
      el("span", { class: "value" }, span(sum(ws, w => w.minutes)))),
    el("ul", { class: "list" }, ws.map(w => el("li", {},
      el("span", { class: "entry" }, person(w.login), el("span", {}, el("a", { href: "#/t/" + w.ticket }, w.ticket), " ", f.byId[w.ticket].title,
        w.note && el("small", {}, " · " + w.note))),
      el("span", { class: "value" }, span(w.minutes)))))))));
}

/** For an administrator: what an hour of each person costs. */
function timeRates(r) {
  if (!ME.admin) return null;
  const logins = [...new Set(r.work.map(w => w.login))].filter(l => !r.agents.includes(l)).sort();
  const own = l => r.rates_own[l] || { rate: 0, currency: r.currency };
  const inputs = logins.map(l => [l, el("input", { type: "number", min: 0, max: 10000, step: "0.5", value: own(l).rate ? own(l).rate / 100 : "", "aria-label": l, placeholder: "0" })]);
  return el("details", { class: "card rates" }, el("summary", {}, t("Hourly rates")),
    el("form", { class: "form", onsubmit: act(async () => {
      for (const [l, i] of inputs) {
        const rate = Math.round(Number(i.value || 0) * 100);
        if (rate !== own(l).rate) await api("api/rates", { login: l, rate });
      }
      await route();
      toast(t("Saved"));
    }) },
      el("p", { class: "meta" }, t("What an hour of each person costs, in the currency each person chose. The page converts it and multiplies it by the time logged. Only administrators and project owners see the amounts. Agents report their own cost.")),
      el("div", { class: "fields" }, inputs.map(([l, i]) => el("label", { class: "field" }, el("span", {}, person(l), " ", el("small", {}, own(l).currency)), i))),
      !logins.length && el("p", { class: "empty" }, t("Nobody logged time yet.")),
      el("button", { class: "primary" }, t("Save"))));
}

let redraw = () => {};
async function timePage(kind, name) {
  const path = kind === "project" ? "api/projects/" + encodeURIComponent(name) + "/time" : "api/spaces/" + encodeURIComponent(name) + "/time";
  const r = await api(path);
  setSpace(r.space);
  const body = el("div", {}), bar = el("div", { class: "row toolbar" });
  const toggle = (values, key, label) => el("span", { class: "toggle", role: "group", "aria-label": t(label) }, Object.keys(values).map(v =>
    el("button", { type: "button", "aria-pressed": String(TIME[key] === v), onclick: () => { TIME[key] = v; saveTime(); redraw(); } }, t(values[v]))));
  redraw = () => {
    const f = timeFigures(r);
    bar.replaceChildren(toggle(PERIODS, "period", "Period"), toggle(VIEWS, "view", "View"));
    const view = { summary: timeSummary, sheet: timeSheet, journal: timeJournal }[TIME.view] || timeSummary;
    body.replaceChildren(timeHero(r, f), ...[view(r, f)].flat(Infinity).filter(Boolean));
  };
  show(
    el("div", { class: "top" },
      el("div", {}, el("h1", {}, t("Time · {name}", { name: r.name || t("No space") })),
        el("p", { class: "meta" }, kind === "project" ? [r.space, name].filter(Boolean).join(" · ") : tp(r.projects.length, "{n} project", "{n} projects"))),
      kind === "project" ? el("a", { class: "button", href: "#/p/" + name }, t("Tickets")) : el("a", { class: "button", href: "#/" }, t("Projects"))),
    bar, body, timeRates(r));
  redraw();
}

/* ---------- agents ---------- */

// What the agents did and do: the runs they report (MCP tool report_run), grouped by piece of
// work. Few things on the page: a sentence, what runs now, one line per piece of work (a click
// opens it), one card per agent. Followed live like the other pages (follow()).
const AGENT_PERIODS = { "1": "24 hours", "7": "7 days", "30": "30 days" };
const AGENTS = { period: "1", pick: "", all: false };
const STALE = 6 * 3600;   // a run still "running" and silent for that long was lost
const nowSec = () => Math.floor(Date.now() / 1000);
const lost = r => r.state === "running" && nowSec() - r.updated > STALE;
const going = r => r.state === "running" && !lost(r);
const finish = r => r.ended || r.updated;
// One colour per agent name, among the six of the charts.
function agentColour(name) {
  let h = 0;
  for (const ch of name) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
  return "k" + (h % 6 + 1);
}
const agentTag = name => el("span", { class: "ag-role " + agentColour(name) }, name);
const lasted = r => clock(Math.max(60, finish(r) - r.started));

// The runs of one ticket make one piece of work; a run without a ticket joins the run that started it.
function pieces(runs) {
  // a run is found by (account, run_id): the parent of a run is one of the same account
  const id = (login, run) => login + "\0" + run, byId = new Map(runs.map(r => [id(r.login, r.run_id), r]));
  const root = r => { for (let i = 0; i < 10 && byId.has(id(r.login, r.parent)); i++) r = byId.get(id(r.login, r.parent)); return r; };
  // the ticket of a run, else the one of the run that started it, and so on up
  const ticketOf = r => { for (let i = 0; r && i < 10; i++, r = byId.get(id(r.login, r.parent))) if (r.ticket) return r.ticket; return ""; };
  const found = new Map();
  for (const r of runs) {
    const key = ticketOf(r) || id(root(r).login, root(r).run_id);
    if (!found.has(key)) found.set(key, { key, runs: [] });
    found.get(key).runs.push(r);
  }
  return [...found.values()].map(g => {
    g.runs.sort((a, b) => a.started - b.started);
    g.start = g.runs[0].started;
    g.last = Math.max(...g.runs.map(r => going(r) ? nowSec() : finish(r)));
    g.title = g.runs[0].task;
    g.ticket = (g.runs.find(r => r.ticket) || {}).ticket || "";
    return g;
  }).sort((a, b) => b.last - a.last);
}

// The verdict of a piece of work: what is going on, else how it ended.
function verdict(g) {
  const rs = g.runs;
  if (rs.some(going)) return ["wait", t("In progress")];
  if (rs.some(r => r.state === "waiting")) return ["warn", t("Waiting")];
  if (rs.some(lost)) return ["unk", t("No news")];
  if (rs.some(r => r.state === "failed")) return ["bad", t("Failed")];
  const said = [...rs].reverse().find(r => r.result);
  // a verdict that starts with a number ("3 gaps") counts something left to fix
  return [said && /^[1-9]\d* /.test(said.result) ? "warn" : "ok", said ? said.result : t("Done")];
}

function runFacts(r) {
  const out = [stamp(r.started), lasted(r), r.model, r.tokens_out != null && t("{n} tokens written", { n: r.tokens_out.toLocaleString(LOCALE()) }),
    r.cost_counted != null && (r.cost_estimated ? t("{a} (estimate)", { a: "≈ " + dollars(r.cost_counted) }) : dollars(r.cost_counted)), r.host];
  return out.filter(Boolean).join(" · ");
}

function agentRow(g, redraw) {
  const open = AGENTS.pick === g.key, span = Math.max(g.last - g.start, 1), [cls, label] = verdict(g);
  const row = el("button", { type: "button", class: "ag-row", "aria-expanded": String(open),
    onclick: () => { AGENTS.pick = open ? "" : g.key; redraw(); } },
    el("span", { class: "who" }, el("b", {}, g.title), el("small", {}, [g.ticket, stamp(g.start)].filter(Boolean).join(" · "))),
    el("span", { class: "ag-track" }, g.runs.map(r => el("span", {
      class: "ag-bar " + agentColour(r.agent), title: r.agent + " · " + lasted(r),
      style: "left:" + (100 * (r.started - g.start) / span).toFixed(2) + "%;width:" + (100 * ((going(r) ? nowSec() : finish(r)) - r.started) / span).toFixed(2) + "%" }))),
    el("span", { class: "pill " + cls }, label));
  if (!open) return row;
  return [row, el("div", { class: "ag-detail" },
    g.runs.map(r => el("div", { class: "ag-step" }, agentTag(r.agent), el("span", {}, r.task), el("span", { class: "m" }, runFacts(r)))),
    g.ticket && el("a", { href: "#/t/" + g.ticket }, t("Open the ticket {id}", { id: g.ticket })))];
}

function agentCards(runs, rates) {
  const names = [...new Set(runs.map(r => r.agent))].sort();
  return names.map(name => {
    const mine = runs.filter(r => r.agent === name), usd = sum(mine, r => r.cost_counted || 0), approx = mine.some(r => r.cost_estimated) ? "≈ " : "";
    const lengths = mine.filter(r => r.ended).map(r => r.ended - r.started).sort((a, b) => a - b);
    const models = {};
    for (const r of mine) if (r.model) models[r.model] = (models[r.model] || 0) + 1;
    const model = Object.keys(models).sort((a, b) => models[b] - models[a])[0];
    const failed = mine.filter(r => r.state === "failed").length;
    const measured = (rates.agents || []).find(a => a.agent === name) || {}, hour = measured.per_hour;
    return el("div", { class: "ag-card" }, agentTag(name),
      el("b", { class: "n" }, tp(mine.length, "{n} run", "{n} runs")),
      el("small", {}, [model, lengths.length && t("median {t}", { t: clock(Math.max(60, lengths[Math.floor(lengths.length / 2)])) })].filter(Boolean).join(" · ")),
      el("small", {}, [usd > 0 && approx + money(usd), failed > 0 && t("{n} failed", { n: failed })].filter(Boolean).join(" · ")),
      el("small", {}, hour != null ? t("{a} an hour over {n} days", { a: (measured.estimated ? "≈ " : "") + fmt(hour / 100, rates.currency), n: rates.days })
        : t("Hourly rate: not measured yet")));
  });
}

async function agentsPage() {
  const path = "api/runs?since=" + (nowSec() - Number(AGENTS.period) * 86400);
  const [data, rates] = await Promise.all([api(path), api("api/agent-rates").catch(() => ({ agents: [] }))]);
  const runs = live(path, data);
  const body = el("div", { class: "agents" });
  const toggle = el("span", { class: "toggle", role: "group", "aria-label": t("Period") }, Object.keys(AGENT_PERIODS).map(v =>
    el("button", { type: "button", "aria-pressed": String(AGENTS.period === v), onclick: () => { AGENTS.period = v; route(); } }, t(AGENT_PERIODS[v]))));
  const redraw = () => {
    const now = runs.filter(going), asked = runs.filter(r => r.state === "waiting").length;
    const closed = runs.filter(r => !going(r) && r.state !== "waiting");
    const tickets = new Set(closed.map(r => r.ticket).filter(Boolean)).size, usd = sum(runs, r => r.cost_counted || 0);
    const approx = runs.some(r => r.cost_estimated) ? "≈ " : "";
    const fact = (label, value) => el("span", { class: "fact" }, el("small", {}, t(label)), el("b", {}, value));
    const work = pieces(runs), shown = AGENTS.all || work.length <= 11 ? work : work.slice(0, 10);
    body.replaceChildren(...[
      el("div", { class: "time-hero" },
        el("p", { class: "big" }, now.length
          ? [el("strong", {}, tp(now.length, "{n} agent at work", "{n} agents at work"))]
          : [el("strong", {}, t("No agent at work"))],
        " ", el("span", {}, AGENTS.period === "1" ? t("in the last 24 hours") : t("in the last {n} days", { n: AGENTS.period }))),
        el("div", { class: "facts" }, fact("Finished", String(closed.length)), fact("Tickets worked", String(tickets)),
          fact("Waiting for an answer", String(asked)), usd > 0 && fact("Cost", approx + money(usd)))),
      runs.length === 0 && el("p", { class: "empty" }, t("No agent reported a run in this period.")),
      now.length > 0 && el("section", {}, el("h2", {}, t("Now")),
        el("div", { class: "ag-now" }, now.slice(0, 6).map(r => el("div", { class: "ag-now-row" },
          el("i", { class: "ag-dot pulse " + agentColour(r.agent) }), agentTag(r.agent), el("b", {}, r.task),
          el("span", { class: "since" }, t("for {t}", { t: clock(Math.max(60, nowSec() - r.started)) })))),
          now.length > 6 && el("p", { class: "meta" }, t("and {n} more", { n: now.length - 6 })))),
      work.length > 0 && el("section", {}, el("h2", {}, t("Work")),
        el("div", { class: "ag-list" }, shown.map(g => agentRow(g, redraw)).flat()),
        shown.length < work.length && el("button", { type: "button", class: "quiet more", onclick: () => { AGENTS.all = true; redraw(); } },
          t("Show all ({n})", { n: work.length }))),
      runs.length > 0 && el("section", {}, el("h2", {}, t("The team")), el("div", { class: "ag-team" }, agentCards(runs, rates)))].filter(Boolean));
  };
  show(el("div", { class: "top" }, el("div", {}, el("h1", {}, t("Agents")),
    el("p", { class: "meta" }, t("What the agents do, how long it takes and what it costs. Each agent reports its runs by itself."))), toggle), body);
  redraw();
}

/* ---------- frame ---------- */

async function route() {
  const h = location.hash.replace(/^#\/?/, "").split("/");
  LIVE = null;
  try {
    if (h[0] === "p" && h[2] === "members") await members(h[1]);
    else if (h[0] === "p" && h[2] === "time") await timePage("project", h[1]);
    else if (h[0] === "e") await timePage("space", decodeURIComponent(h.slice(1).join("/")));
    else if (h[0] === "p" && h[1]) await project(h[1]);
    else if (h[0] === "t" && h[1]) await ticket(h[1]);
    else if (h[0] === "n") await notices();
    else if (h[0] === "m") await mine();
    else if (h[0] === "q") await search();
    else if (h[0] === "a") await agentsPage();
    else if (h[0] === "s") await settings();
    else if (h[0] === "agent") await agentPage();
    else if (h[0] === "u") await profile();
    else return await home();
    await nav();
  } catch (e) {
    show(el("p", { class: "banner" }, e.message), el("p", {}, el("a", { href: "#/" }, t("Back to the projects"))));
  } finally {
    bell();
  }
}

function frame() {
  $("#who").textContent = ME.name || ME.login;
  $("#me").replaceChildren(avatar(ME.login), t("My profile"));
  $("#version").textContent = "Cairn Plan " + ME.version;
  $("#lang").textContent = t("Language: {name}", { name: LANGS[LANG].name });
  $("#settings").textContent = t("Settings");
  $("#settings").hidden = !ME.admin;
}

addEventListener("hashchange", route);
addEventListener("DOMContentLoaded", act(async () => {
  ME = await api("api/me");
  ME.photos ||= {};
  LANG = pickLang(ME.lang);
  document.documentElement.lang = LANG;
  $("#lang").addEventListener("click", () => {
    LANG = LANG === "fr" ? "en" : "fr";
    try { localStorage.setItem("cairn-lang", LANG); } catch (e) { /* storage blocked */ }
    document.documentElement.lang = LANG;
    frame();
    route();
  });
  frame();
  await route();
}));
