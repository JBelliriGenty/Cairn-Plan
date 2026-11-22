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

async function api(path, body) {
  const r = await fetch(path, body === undefined ? { redirect: "manual" } : {
    redirect: "manual", method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body),
  });
  // Sign-in lost (session ended at the proxy): the proxy answers with a
  // redirect to its sign-in page, which a background request cannot follow.
  // Reload the page so that the browser itself goes through the sign-in.
  if (r.type === "opaqueredirect" || r.status === 401) {
    let last = 0;
    try { last = Number(sessionStorage.getItem("cairn-plan-reload")) || 0; } catch (e) { /* storage blocked */ }
    if (Date.now() - last > 15000) {   // never in a loop
      try { sessionStorage.setItem("cairn-plan-reload", String(Date.now())); } catch (e) { /* storage blocked */ }
      location.reload();
      await new Promise(() => {});   // nothing more to do in this page
    }
    throw new Error(t("Your session has ended. Reload the page to sign in again."));
  }
  const j = await r.json().catch(() => ({ error: "request failed" }));
  if (!r.ok) throw new Error(t(j.error || "request failed"));
  return j;
}

// Sends one file as it is (not JSON). The server names the limit; checking here spares a long wait.
async function upload(path, file) {
  if (file.size > ME.max_file_mb * 1048576) throw new Error(t("file too large"));
  const r = await fetch(path, { redirect: "manual", method: "POST", headers: { "Content-Type": "application/octet-stream" }, body: file });
  if (r.type === "opaqueredirect" || r.status === 401) throw new Error(t("Your session has ended. Reload the page to sign in again."));
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
const FIELD = { title: "Title", state: "State", priority: "Priority", assignee: "Assignee", start_date: "Start", end_date: "End", milestone: "Milestone", agent: "Agent" };

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
        el("a", { href: "#/p/" + p.key, ...(p.key === hereKey ? { "aria-current": "page" } : {}) }, p.name)))),
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
  const title = el("input", { required: true, maxLength: 200, placeholder: t("Title of the ticket"), "aria-label": t("Title of the ticket") });
  const cell = (label, ...kids) => el("td", { "data-label": t(label) }, kids);
  const prio = k => k.priority !== "none" && el("span", { class: "pill prio-" + k.priority }, t(PRIORITY[k.priority]));
  const agent = k => k.agent && el("span", { class: "pill agent" }, k.agent);
  const tickets = el("div", {}), count = el("span", { class: "label" });

  const table = shown => el("div", { class: "card table-wrap" }, shown.length ? el("table", {},
    el("thead", {}, el("tr", {}, ["Ticket", "Title", "State", ...(stones.length ? ["Milestone"] : []), "Priority", "Assignee", "Start", "End"].map(h => el("th", {}, t(h))))),
    el("tbody", {}, shown.map(k => el("tr", {},
      cell("Ticket", el("a", { href: "#/t/" + k.id }, k.id)),
      cell("Title", el("a", { href: "#/t/" + k.id }, k.title), k.parent && el("span", { class: "label" }, " · " + k.parent), " ", agent(k), progress(k)),
      cell("State", pill(k.state)),
      stones.length > 0 && cell("Milestone", k.milestone),
      cell("Priority", prio(k)),
      cell("Assignee", k.assignee),
      cell("Start", day(k.start_date)),
      cell("End", day(k.end_date))))))
    : el("p", { class: "empty" }, t("No ticket here.")));

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
          el("span", { class: "kmeta" }, prio(k), agent(k), k.assignee && el("span", { class: "label" }, k.assignee),
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
      el("a", { class: "button", href: "#/p/" + p.key + "/members" }, tp(p.members.length, "{n} member", "{n} members"))),
    el("div", { class: "row toolbar" },
      el("span", { class: "toggle" }, viewButton("list", "List"), viewButton("board", "Board")),
      el("input", { type: "search", value: FILTERS.q, placeholder: t("Search a ticket"), "aria-label": t("Search a ticket"),
        oninput: ev => { FILTERS.q = ev.target.value; draw(); } }),
      stateFilter,
      select(["*", "", ...p.members.map(m => m.login)], FILTERS.assignee, v => v === "*" ? t("Everyone") : v || t("Unassigned"),
        { "aria-label": t("Assignee"), onchange: ev => { FILTERS.assignee = ev.target.value; draw(); } }),
      select(["*", ...ME.priorities], FILTERS.priority, v => v === "*" ? t("All priorities") : t(PRIORITY[v]),
        { "aria-label": t("Priority"), onchange: ev => { FILTERS.priority = ev.target.value; draw(); } }),
      stones.length > 0 && select(["*", "", ...stones], FILTERS.milestone, v => v === "*" ? t("All milestones") : v || t("No milestone"),
        { "aria-label": t("Milestone"), class: "wide-select", onchange: ev => { FILTERS.milestone = ev.target.value; draw(); } }),
      count),
    tickets,
    hint,
    canEdit && el("form", { class: "card form", onsubmit: act(async () => {
      const r = await api("api/projects/" + encodeURIComponent(key) + "/tickets", { title: title.value });
      location.hash = "#/t/" + r.id;
    }) },
      el("h2", {}, t("New ticket")),
      el("div", { class: "row" }, title, el("button", { class: "primary" }, t("Add the ticket"))),
      el("small", {}, t("A new ticket starts as a proposal; a project owner approves it."))),
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
      el("a", { class: "button", href: "#/p/" + p.key }, t("Back to the project"))),
    el("div", { class: "card table-wrap" }, el("table", {},
      el("thead", {}, el("tr", {}, ["Login", "Name", "Role", ""].map(h => el("th", {}, h && t(h))))),
      el("tbody", {}, p.members.map(m => el("tr", {},
        el("td", { "data-label": t("Login") }, m.login),
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

async function mine() {
  const ks = live("api/mine", await api("api/mine"));
  const row = k => el("li", {},
    el("a", { href: "#/t/" + k.id },
      el("strong", {}, k.id + " · " + k.title),
      el("span", { class: "label" }, [k.space, k.project_name, t("by {author}", { author: k.author })].filter(Boolean).join(" · "))),
    pill(k.state));
  const asked = ks.filter(k => k.state === "waiting"), rest = ks.filter(k => k.state !== "waiting");
  show(
    el("div", { class: "top" }, el("div", {}, el("h1", {}, t("For me")),
      el("p", { class: "meta" }, t("The open tickets assigned to you, in every space.")))),
    el("h2", {}, t("Waiting for your answer")),
    el("div", { class: "card" }, asked.length ? el("ul", { class: "list notices" }, asked.map(row))
      : el("p", { class: "empty" }, t("Nothing waits for you."))),
    rest.length > 0 && el("h2", {}, t("Also assigned to you")),
    rest.length > 0 && el("div", { class: "card" }, el("ul", { class: "list notices" }, rest.map(row))),
  );
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
    milestone: el("input", { value: tk.milestone, maxLength: 80, disabled: !canEdit }),
    // which agent builds it: part of what is approved, frozen like the text
    agent: el("input", { value: tk.agent, maxLength: 20, pattern: "[a-z0-9._\\-]*", autocapitalize: "none", disabled: frozen }),
  };
  f.milestone.setAttribute("list", "milestones");   // "list" is read-only as a property
  const save = async extra => {
    const body = { ...extra };
    for (const [k, c] of Object.entries(f)) if (c.value.trim() !== tk[k] && !(k in body)) body[k] = c.value;
    await api(path, body);
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
  const words = el("textarea", { required: true, rows: 3, maxLength: 10000, placeholder: t("Write a comment") });
  // a question to the person in front: what is asked, then one click to answer
  const asked = tk.state === "waiting" && tk.assignee === ME.login;
  const answer = el("textarea", { rows: 3, maxLength: 10000, placeholder: t("Your answer (optional)"), "aria-label": t("Your answer") });
  const reply = state => act(async () => {
    if (answer.value.trim() || !state) await api(path + "/comments", { body: answer.value });   // empty and alone: refused
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
      el("div", { class: "form" }, answer,
        el("div", { class: "row" },
          el("button", { type: "button", class: "approve", onclick: reply("done") }, t("Done, close it")),
          owner && el("button", { type: "button", onclick: reply("todo") }, t("Answered: carry on")),
          el("button", { type: "button", class: "quiet", onclick: reply("") }, t("Only send the answer"))),
        el("small", {}, t(owner ? "Done: nothing more to do. Carry on: you approve what the ticket says comes next, with your answer."
          : "Done: nothing more to do.")))),
    tk.state === "backlog" && el("p", { class: "note" }, owner
      ? t("Proposal: approve it to mark it as ready to be done.")
      : t("This ticket waits for the approval of a project owner.")),
    tk.state !== "backlog" && canEdit && !owner && el("p", { class: "note" },
      t("Approved: only a project owner can change its title and description.")),
    el("form", { class: "card form", onsubmit: act(() => save()) },
      field("Title", f.title),
      field("Description", f.description),
      el("div", { class: "fields" },
        field("State", f.state), field("Priority", f.priority), field("Assignee", f.assignee),
        field("Start", f.start_date), field("End", f.end_date),
        field("Milestone", f.milestone, "A family of tickets: a phase, a version."),
        field("Agent", f.agent, "The agent role that builds it (dev, doc); empty: the project lead.")),
      el("datalist", { id: "milestones" }, [...new Set(p.tickets.map(k => k.milestone).filter(Boolean))].sort().map(s => el("option", { value: s }))),
      canEdit && el("div", { class: "row" },
        el("button", { class: "primary" }, t("Save")),
        owner && tk.state === "backlog" && el("button", { type: "button", class: "approve", onclick: act(() => save({ state: "todo" })) }, t("Approve")))),
    tk.children.length > 0 && el("h2", {}, t("Sub-tickets"), " ", progress(tk)),
    tk.children.length > 0 && el("div", { class: "card" }, el("ul", { class: "list" }, tk.children.map(k => el("li", {},
      el("a", { href: "#/t/" + k.id }, el("strong", {}, k.id + " · " + k.title)),
      el("span", { class: "kmeta" }, k.agent && el("span", { class: "pill agent" }, k.agent),
        k.assignee && el("span", { class: "label" }, k.assignee), pill(k.state)))))),
    el("h2", {}, t("Links")),
    el("div", { class: "card" },
      tk.links.length ? el("ul", { class: "list" }, tk.links.map(l => el("li", {},
        el("a", { href: l.url, target: "_blank", rel: "noopener noreferrer" }, l.title || l.url),
        canEdit && el("button", { type: "button", class: "quiet", onclick: act(async () => { await api(path + "/links", { delete: l.id }); await ticket(id); }) }, t("Remove")))))
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
        el("p", { class: "meta" }, c.author + " · " + stamp(c.at)), rich(c.body)))
        : el("p", { class: "empty" }, t("No comment yet.")),
      el("form", { class: "form", onsubmit: act(async () => { await api(path + "/comments", { body: words.value }); await ticket(id); }) },
        words, el("div", { class: "row" }, el("button", {}, t("Send"))))),
    el("h2", {}, t("History")),
    el("div", { class: "card" }, el("ul", { class: "list history" }, tk.events.map(e => el("li", {},
      el("span", {}, e.actor + " " + describe(e)), el("span", { class: "label" }, stamp(e.at)))))),
  );
}

/* ---------- notifications ---------- */

const NOTICE = { comment: "{actor} commented", state: "{actor} moved the ticket to {state}", assigned: "{actor} assigned the ticket to you" };
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
  let n;
  try {
    const r = await fetch("api/notifications", { redirect: "manual" });
    if (!r.ok) return;
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
    if (!r.ok) return;
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
    el("div", { class: "card" }, n.items.length ? el("ul", { class: "list notices" }, n.items.map(i => el("li", { class: i.read ? "" : "unread" },
      el("a", { href: "#/t/" + i.ticket },
        el("strong", {}, i.ticket + " · " + i.title),
        el("span", {}, said(i)),
        i.kind === "comment" && el("span", { class: "excerpt" }, i.detail)),
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

/* ---------- settings (administrators) ---------- */

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
      el("p", { class: "meta" }, t("Called as soon as a ticket is approved, and when whoever approved a waiting ticket answers it: an agent can then take the ticket at once.")),
      el("div", { class: "fields" },
        field("Webhook address", f.agent_hook_url, "Empty: nothing is called."),
        field("Secret", f.agent_hook_secret, "16 characters or more, the same as in the agent. It signs each call. Leave empty to keep the one that is saved.")),
      el("div", { class: "row" }, el("button", { class: "primary" }, t("Save")))),
    el("div", { class: "card form" },
      el("h2", {}, t("Try it")),
      el("p", { class: "meta" }, t("Sends one message to you with the settings that are saved. The answer of the server is shown if it fails.")),
      el("div", { class: "row" }, el("span", {}, t("E-mail")), test("mail"), el("span", {}, t("Phone (ntfy)")), test("ntfy"), el("span", {}, t("Agent")), test("hook"))),
  );
}

/* ---------- frame ---------- */

async function route() {
  const h = location.hash.replace(/^#\/?/, "").split("/");
  LIVE = null;
  try {
    if (h[0] === "p" && h[2] === "members") await members(h[1]);
    else if (h[0] === "p" && h[1]) await project(h[1]);
    else if (h[0] === "t" && h[1]) await ticket(h[1]);
    else if (h[0] === "n") await notices();
    else if (h[0] === "m") await mine();
    else if (h[0] === "s") await settings();
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
  $("#version").textContent = "Cairn Plan " + ME.version;
  $("#lang").textContent = t("Language: {name}", { name: LANGS[LANG].name });
  $("#settings").textContent = t("Settings");
  $("#settings").hidden = !ME.admin;
}

addEventListener("hashchange", route);
addEventListener("DOMContentLoaded", act(async () => {
  ME = await api("api/me");
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
