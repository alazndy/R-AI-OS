/* Product-design preview. No network, process execution, or persisted state. */
"use strict";

const $ = (selector) => document.querySelector(selector);
const $$ = (selector) => [...document.querySelectorAll(selector)];
const files = {
  handoff: {name: "wf_handoff.rs", path: "crates / raios-core / src / db / wf_handoff.rs", language: "Rust", lines: [
    "// Design excerpt: project-scoped handoff lookup.",
    "// Illustrative contract; not production source.",
    "", "pub fn pending_for_project(",
    "    conn: &Connection,", "    project_root: &Path,", "    recipient: &str,",
    ") -> Result<Option<HandoffContext>> {",
    "    let context = cp_take_pending_handoff_for_path(",
    "        conn,", "        project_root,", "        recipient,", "    )?;", "",
    "    // Never take another project's pending context.",
    "    Ok(context)", "}", "",
    "// Delivery is a separate event from queueing.",
    "// Spawn failure keeps the pending context available.",
  ]},
  runner: {name: "agent_runner.rs", path: "crates / raios-runtime / src / agent_runner.rs", language: "Rust", lines: [
    "// Design excerpt: receiver launch lifecycle.", "// Illustrative contract; not production source.", "",
    "fn launch_receiver(project: &Project, agent: Agent) {",
    "    let context = pending_for_project(project, agent);",
    "    let attempt = record_launch_attempt(project, agent);", "",
    "    match spawn_with_context(project, agent, context) {",
    "        Ok(process) => record_started(attempt, process),",
    "        Err(error) => record_launch_failed(attempt, error),",
    "    }", "}", "", "// Started does not prove context was read.",
    "// Keep the failure reason and retry scope visible.",
  ]},
  command: {name: "command.rs", path: "crates / raios-contracts / src / command.rs", language: "Rust", lines: [
    "// Existing command surface, shortened for preview.", "pub enum Command {",
    "    LaunchAgent {", "        agent_name: String,", "        project_path: String,",
    "        prompt: Option<String>,", "        idempotency_key: String,", "    },",
    "    CancelAgentRun {", "        run_id: String,", "        idempotency_key: String,", "    },",
    "}", "", "// Terminal views attach to runs; they do not own tasks.",
  ]},
  design: {name: "product-design.md", path: "docs / design / workspace-v1 / product-design.md · örnek içerik", language: "Markdown", lines: [
    "# RAIOS Workspace", "", "A desktop workspace with RAIOS at its foundation.", "",
    "## Shared project context", "- Editor documents", "- Agent terminal sessions", "- Tasks and handoff receipts", "- Memory, approvals, and activity", "",
    "## First usable slice", "Open project → inspect code → start agent", "→ review change → hand off → verify receipt.",
  ]},
  memory: {name: "memory.md", path: "R-AI-OS / memory.md · örnek içerik", language: "Markdown", lines: [
    "# Project Memory: R-AI-OS", "", "## Current Focus", "- Keep handoffs within their project root.", "- Show launch and delivery as separate stages.", "",
    "## Technical Decisions", "- RAIOS is the shared orchestration foundation.", "- The workspace presents code and agents together.", "- Preserve pending context when launch fails.", "",
    "## Next Action", "- Review the receiving agent's delivery evidence.",
  ]},
  readme: {name: "README.md", path: "R-AI-OS / README.md · örnek içerik", language: "Markdown", lines: [
    "# RAIOS Workspace", "", "A local development workspace built around R-AI-OS.", "",
    "## Main areas", "- Project files", "- Editor and diff", "- Agent terminals", "- Task, memory, and approval inspector", "",
    "This is a product-design preview.", "No command or agent runs in this browser page.",
  ]},
};

const state = {file: "handoff", layout: "balanced", connected: true, decision: null,
  codexStopped: false, recipient: "Claude", recipientState: "idle", stage: "queued", attempt: 1};

function announce(message) {
  $("#notice").textContent = message;
}

function log(message) {
  const entry = document.createElement("li");
  const caption = document.createElement("span");
  caption.className = "muted";
  caption.textContent = "Önizleme işlemi";
  const content = document.createElement("p");
  content.textContent = message;
  entry.append(caption, content);
  $("#activity").prepend(entry);
  announce(message);
}

function codeTokens(line) {
  const text = document.createDocumentFragment();
  if (line.trim().startsWith("//")) {
    const comment = document.createElement("span");
    comment.className = "code-comment";
    comment.textContent = line;
    text.append(comment);
    return text;
  }
  const tokens = line.split(/(\b(?:pub|fn|let|match|enum|String|Option|Result|Path|Connection|HandoffContext|Agent|Project)\b)/g);
  tokens.forEach((token) => {
    const span = document.createElement("span");
    if (/^(pub|fn|let|match|enum)$/.test(token)) span.className = "code-keyword";
    else if (/^(String|Option|Result|Path|Connection|HandoffContext|Agent|Project)$/.test(token)) span.className = "code-type";
    span.textContent = token;
    text.append(span);
  });
  return text;
}

function openFile(key) {
  state.file = key;
  const file = files[key];
  $("#code").replaceChildren();
  file.lines.forEach((line, index) => {
    const row = document.createElement("span");
    row.className = "code-line";
    const number = document.createElement("span");
    number.className = "line-number";
    number.setAttribute("aria-hidden", "true");
    number.textContent = index + 1;
    row.append(number, codeTokens(line));
    $("#code").append(row);
  });
  $("#code-tab").textContent = file.name + (["handoff", "runner"].includes(key) ? " · M" : "");
  $("#breadcrumb").textContent = file.path;
  $("#editor-caption").textContent = file.language + " · örnek kod";
  $$("[data-file]").forEach((button) => {
    button.classList.toggle("selected", button.dataset.file === key);
    if (button.dataset.file === key) button.setAttribute("aria-current", "true");
    else button.removeAttribute("aria-current");
  });
  showDiff(false);
}

function showDiff(visible) {
  $("#code").hidden = visible;
  $("#diff").hidden = !visible;
  $("#code-tab").classList.toggle("active", !visible);
  $("#diff-tab").classList.toggle("active", visible);
  $("#code-tab").setAttribute("aria-pressed", !visible);
  $("#diff-tab").setAttribute("aria-pressed", visible);
  $("#breadcrumb").textContent = visible ? "İnceleme / wf_handoff.rs / istek #A17 · örnek revizyon 1" : files[state.file].path;
  $("#editor-caption").textContent = visible ? "Değişiklik önizlemesi · kavramsal diff" : files[state.file].language + " · örnek kod";
}

function setLayout(layout) {
  state.layout = layout;
  $(".workspace").dataset.layout = layout;
  $$("button[data-layout]").forEach((button) => button.setAttribute("aria-pressed", button.dataset.layout === layout));
  const labels = {balanced: "Dengeli", editor: "Editör", agents: "Ajan"};
  $("#status-layout").textContent = labels[layout] + " yerleşimi";
  announce(labels[layout] + " yerleşimi seçildi; proje ve oturumlar aynı kaldı.");
}

function setInspector(panel, focus = false) {
  $$("[data-inspector]").forEach((button) => {
    const selected = button.dataset.inspector === panel;
    button.setAttribute("aria-selected", selected);
    button.tabIndex = selected ? 0 : -1;
    $("#panel-" + button.dataset.inspector).hidden = !selected;
    if (selected && focus) button.focus();
  });
}

function modal(id) {
  if (!state.connected && id !== "palette-dialog") return;
  $("#" + id).showModal();
}

function refreshControls() {
  $$("[data-runtime]").forEach((button) => {
    button.disabled = !state.connected;
    button.title = state.connected ? "" : "Raios bağlantısı olmadan bu işlem yapılamaz.";
  });
  $("#stop-codex").disabled = !state.connected || state.codexStopped;
  $("#review-approve").disabled = !state.connected || Boolean(state.decision);
  $("#review-reject").disabled = !state.connected || Boolean(state.decision);
  // A run already exists; do not offer an accidental duplicate launch.
  const recipientActive = ["started", "delivered", "standalone"].includes(state.recipientState);
  $("#start-recipient").disabled = !state.connected || recipientActive;
  $("#launch-open").disabled = !state.connected || recipientActive;
  $("#handoff-open").disabled = !state.connected || recipientActive || state.codexStopped;
  if (state.connected && recipientActive) {
    $("#launch-open").title = "Örnek alıcı oturumu açık. Yeni bir deneme için önce durdur.";
    $("#handoff-open").title = "Alıcı açık; yinelenen başlatmayı önlemek için önce oturumu durdur.";
  }
}

function setConnection(connected) {
  state.connected = connected;
  $("#offline").hidden = connected;
  $("#connection").textContent = connected ? "Raios bağlı" : "Bağlantı kesildi";
  $("#connection").setAttribute("aria-pressed", !connected);
  $("#connection").title = connected ? "Bağlantı kesilmesi senaryosunu göster" : "Bağlantıyı geri getir";
  refreshControls();
  log(connected ? "Bağlantı geri geldi (örnek); proje görünümü uzlaştırıldı." : "Bağlantı kesildi (örnek); yeni çalıştırma ve karar işlemleri duraklatıldı.");
}

function recipientOutput(lines) {
  $("#recipient-body").replaceChildren();
  lines.forEach(([text, className = ""]) => {
    const line = document.createElement("p");
    line.textContent = text;
    line.className = className;
    $("#recipient-body").append(line);
  });
}

function renderReceipt() {
  const config = {
    queued: {label: "Kuyrukta", detail: "Kuyruğa almak alıcıyı çalıştırmaz.", steps: [["Kuyruğa alındı", "current"], ["Alıcı başlatılmadı", ""], ["Bağlam teslimi bekleniyor", ""]]},
    started: {label: "Teslim bekliyor", detail: "Alıcı başladı. Bağlamın okunduğu henüz doğrulanmadı. Teslim ve hata durumları için örnek senaryoları kullan.", steps: [["Kuyruğa alındı", "done"], ["Alıcı başlatıldı", "done"], ["Teslim doğrulanmadı", "current"]]},
    delivered: {label: "Teslim edildi · örnek", detail: "Bu senaryo teslim teyidi destekleyen bir ajanı gösterir. Gerçek model teyidi değildir; görev sonucu hâlâ bekleniyor.", steps: [["Kuyruğa alındı", "done"], ["Alıcı başlatıldı", "done"], ["Bağlam teslimi teyit edildi · örnek", "done"]]},
    failed: {label: "Başlatılamadı", detail: "Örnek hata: alıcı CLI bulunamadı. Bağlam kuyrukta korundu. Kurulum kontrolünden sonra tekrar dene.", steps: [["Kuyruğa alındı", "done"], ["Başlatma başarısız", "failed"], ["Bağlam kuyrukta korundu", "current"]]},
    stopped: {label: "Alıcı durduruldu", detail: "Örnek oturum kapatıldı. Önceki teslim olayları akışta kaldı; görev tamamlandı sayılmadı.", steps: [["Handoff geçmişi korundu", "done"], ["Alıcı süreç durduruldu · örnek", "current"], ["Görev sonucu bekleniyor", ""]]},
  }[state.stage];
  $("#receipt-label").textContent = config.label;
  $("#receipt-label").className = state.stage === "failed" ? "error" : state.stage === "delivered" ? "success" : "waiting";
  $("#receipt-detail").textContent = config.detail;
  $("#receipt").replaceChildren();
  config.steps.forEach(([text, className]) => {
    const step = document.createElement("li");
    step.textContent = text;
    step.className = className;
    $("#receipt").append(step);
  });
  $("#start-recipient").hidden = !["queued", "failed"].includes(state.stage);
  $("#start-recipient").textContent = state.stage === "failed" ? "Yeniden dene (örnek)" : "Alıcıyı başlat (örnek)";
  $("#delivery-scenario").hidden = state.stage !== "started";
  $("#failure-scenario").hidden = state.stage !== "started";
}

function startRecipient() {
  if (!state.connected || !["queued", "failed"].includes(state.stage) || ["started", "delivered", "standalone"].includes(state.recipientState)) return;
  if (state.stage === "failed") {
    state.attempt += 1;
    $("#handoff-route").textContent = "Codex → " + state.recipient + " · deneme " + state.attempt;
  }
  state.stage = "started";
  state.recipientState = "started";
  $("#recipient-name").textContent = state.recipient;
  $("#recipient-status").textContent = "Teslim doğrulanmadı";
  $("#recipient-status").className = "waiting";
  $("#stop-recipient").hidden = false;
  const harness = {Claude: "claude", OpenCode: "opencode", Antigravity: "agy"}[state.recipient];
  recipientOutput([["raios run " + harness + " · R-AI-OS", "muted"], ["Örnek alıcı süreç başlatıldı."], ["Görev #42 · proje bağlamı hazır."], ["Bağlamın okunduğuna dair teyit bekleniyor.", "waiting"], ["Teslim senaryosunu sağ panelden seç.", "muted"]]);
  if (state.layout === "editor") setLayout("balanced");
  renderReceipt();
  refreshControls();
  log(state.recipient + " başlatıldı (örnek). Bağlam teslimi ayrı bir teyit bekliyor.");
}

function deliveryScenario() {
  if (!state.connected || state.stage !== "started") return;
  state.stage = "delivered";
  state.recipientState = "delivered";
  $("#recipient-status").textContent = "Çalışıyor · örnek";
  $("#recipient-status").className = "success";
  recipientOutput([["R-AI-OS · görev #42", "muted"], ["✓ Bağlam teslimi teyit edildi (örnek).", "terminal-success"], ["Başlatma hatası yolunu inceliyorum."], ["Görev sonucu henüz verilmedi.", "muted"]]);
  renderReceipt();
  refreshControls();
  log("Teslim teyidi senaryosu gösterildi. Görev tamamlandı sayılmadı.");
}

function failureScenario() {
  if (!state.connected || state.stage !== "started") return;
  state.stage = "failed";
  state.recipientState = "failed";
  $("#recipient-status").textContent = "Başlatılamadı";
  $("#recipient-status").className = "error";
  $("#stop-recipient").hidden = true;
  recipientOutput([["Başlatma hatası senaryosu", "muted"], ["Alıcı CLI bulunamadı (örnek).", "error"], ["Önceki süreç başlangıcı bu senaryo için geri alındı.", "muted"], ["Handoff kuyrukta korundu."], ["Sağ panelden yeniden dene.", "waiting"]]);
  renderReceipt();
  refreshControls();
  log("Başlatma hatası senaryosu: alıcı başlamadı; handoff kuyrukta korundu.");
}

function decisionModal(decision) {
  if (!state.connected || state.decision) return;
  $("#decision-dialog").dataset.decision = decision;
  $("#decision-summary").textContent = decision === "approved" ? "Bu dosya değişikliğini tek seferlik onayla." : "Bu dosya değişikliğini reddet; gerekçeyi ajana görünür kıl.";
  $("#decision-submit").textContent = decision === "approved" ? "Bu isteği onayla (örnek)" : "Bu isteği reddet (örnek)";
  modal("decision-dialog");
}

const paletteCommands = [
  ...Object.keys(files).map((key) => ({label: files[key].name, kind: "Dosya", run: () => openFile(key)})),
  {label: "Dengeli yerleşim", kind: "Görünüm", run: () => setLayout("balanced")},
  {label: "Editöre odaklan", kind: "Görünüm", run: () => setLayout("editor")},
  {label: "Ajanlara odaklan", kind: "Görünüm", run: () => setLayout("agents")},
  {label: "Handoff oluştur", kind: "Görev", run: () => modal("handoff-dialog")},
  {label: "Hafızayı görüntüle", kind: "Bağlam", run: () => setInspector("memory")},
];

function renderPalette() {
  const query = $("#palette-input").value.toLocaleLowerCase("tr");
  const matches = paletteCommands.filter((item) => item.label.toLocaleLowerCase("tr").includes(query));
  $("#palette-results").replaceChildren();
  matches.forEach((command) => {
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = command.label;
    const kind = document.createElement("small");
    kind.textContent = command.kind;
    button.append(kind);
    if (command.kind === "Görev" && (!state.connected || state.codexStopped || ["started", "delivered", "standalone"].includes(state.recipientState))) {
      button.disabled = true;
      button.title = "Bağlantı ve uygun bir kaynak/alıcı oturumu gerekli.";
    }
    button.addEventListener("click", () => {
      $("#palette-dialog").close();
      command.run();
    });
    $("#palette-results").append(button);
  });
  if (!matches.length) {
    const empty = document.createElement("p");
    empty.className = "muted";
    empty.textContent = "Bu taslakta eşleşen dosya veya komut yok.";
    $("#palette-results").append(empty);
  }
}

function openPalette() {
  $("#palette-input").value = "";
  renderPalette();
  modal("palette-dialog");
  $("#palette-input").focus();
}

$$("[data-close]").forEach((button) => button.addEventListener("click", () => button.closest("dialog").close()));
$$("[data-file]").forEach((button) => button.addEventListener("click", () => openFile(button.dataset.file)));
$$("button[data-layout]").forEach((button) => button.addEventListener("click", () => setLayout(button.dataset.layout)));
$$("[data-inspector]").forEach((button, index, tabs) => {
  button.addEventListener("click", () => setInspector(button.dataset.inspector));
  button.addEventListener("keydown", (event) => {
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    const next = event.key === "Home" ? 0 : event.key === "End" ? tabs.length - 1 : (index + (event.key === "ArrowRight" ? 1 : -1) + tabs.length) % tabs.length;
    setInspector(tabs[next].dataset.inspector, true);
  });
});
$("#code-tab").addEventListener("click", () => showDiff(false));
$("#diff-tab").addEventListener("click", () => showDiff(true));
$("#review-open").addEventListener("click", () => {
  if (state.layout === "agents") setLayout("balanced");
  showDiff(true);
  $(".editor-content").focus();
  announce("İstek #A17 için örnek diff açıldı. Karar yalnızca gösterilen revizyona uygulanır.");
});
$("#review-approve").addEventListener("click", () => decisionModal("approved"));
$("#review-reject").addEventListener("click", () => decisionModal("rejected"));
$("#memory-open").addEventListener("click", () => setInspector("memory"));
$("#memory-file-open").addEventListener("click", () => openFile("memory"));
$("#handoff-open").addEventListener("click", () => modal("handoff-dialog"));
$("#launch-open").addEventListener("click", () => modal("launch-dialog"));
$("#start-recipient").addEventListener("click", startRecipient);
$("#delivery-scenario").addEventListener("click", deliveryScenario);
$("#failure-scenario").addEventListener("click", failureScenario);
$("#connection").addEventListener("click", () => setConnection(!state.connected));
$("#reconnect").addEventListener("click", () => setConnection(true));
$("#theme").addEventListener("click", () => {
  const light = document.body.classList.toggle("light");
  $("#theme").textContent = light ? "Koyu tema" : "Açık tema";
  $("#theme").setAttribute("aria-label", light ? "Koyu temaya geç" : "Açık temaya geç");
});
$("#reset").addEventListener("click", () => location.reload());
$("#palette-open").addEventListener("click", openPalette);
$("#palette-input").addEventListener("input", renderPalette);
$("#palette-input").addEventListener("keydown", (event) => {
  if (event.key === "ArrowDown") {
    event.preventDefault();
    $("#palette-results button:not(:disabled)")?.focus();
  } else if (event.key === "Enter") {
    event.preventDefault();
    $("#palette-results button:not(:disabled)")?.click();
  }
});

$("#handoff-form").addEventListener("submit", (event) => {
  event.preventDefault();
  if (!state.connected || ["started", "delivered", "standalone"].includes(state.recipientState) || state.codexStopped) return;
  state.recipient = $("#handoff-agent").value;
  state.stage = "queued";
  state.recipientState = "idle";
  state.attempt += 1;
  $("#handoff-route").textContent = "Codex → " + state.recipient + " · deneme " + state.attempt;
  $("#recipient-name").textContent = state.recipient;
  $("#recipient-status").textContent = "Başlamadı";
  $("#recipient-status").className = "muted";
  $("#stop-recipient").hidden = true;
  recipientOutput([["R-AI-OS · görev #42", "muted"], ["Handoff kuyrukta. Alıcı başlamadı."], [$("#handoff-summary").value]]);
  $("#handoff-dialog").close();
  setInspector("task");
  renderReceipt();
  refreshControls();
  log(state.recipient + " için handoff oluşturuldu (örnek); proje R-AI-OS, görev #42.");
  if ($("#handoff-run").checked) startRecipient();
});

$("#launch-form").addEventListener("submit", (event) => {
  event.preventDefault();
  if (!state.connected || ["started", "delivered", "standalone"].includes(state.recipientState)) return;
  state.recipient = $("#launch-agent").value;
  state.recipientState = "standalone";
  $("#recipient-name").textContent = state.recipient;
  $("#recipient-status").textContent = "Çalışıyor · örnek";
  $("#recipient-status").className = "success";
  $("#stop-recipient").hidden = false;
  recipientOutput([["Yeni örnek oturum · R-AI-OS", "muted"], ["› " + $("#launch-prompt").value], ["Bu başlatma bekleyen handoff’u tüketmez.", "waiting"]]);
  $("#launch-dialog").close();
  if (state.layout === "editor") setLayout("balanced");
  refreshControls();
  log(state.recipient + " için bağımsız örnek oturum açıldı. Önceki handoff makbuzu korundu.");
});

$("#decision-form").addEventListener("submit", (event) => {
  event.preventDefault();
  if (!state.connected || state.decision) return;
  state.decision = $("#decision-dialog").dataset.decision;
  const approved = state.decision === "approved";
  $("#approval-section h3").textContent = approved ? "Değişiklik onaylandı" : "Değişiklik reddedildi";
  $("#approval-section .section-title span").textContent = "Örnek karar";
  $("#diff-status").textContent = approved ? "Onaylandı · örnek" : "Reddedildi · örnek";
  $("#task-state").textContent = approved ? "İnceleme sürüyor" : "Revizyon bekliyor";
  $("#codex-status").textContent = state.codexStopped ? "Durduruldu" : approved ? "Çalışıyor · örnek" : "Revizyon gerekli";
  $("#codex-status").className = approved ? "success" : "waiting";
  $("#codex-output").textContent = approved ? "Örnek onay alındı; görev sonucu bekleniyor." : "Örnek değişiklik reddedildi; revizyon gerekli.";
  $("#decision-dialog").close();
  refreshControls();
  log("İstek #A17 " + (approved ? "onaylandı" : "reddedildi") + " (örnek). " + $("#decision-reason").value);
});

$("#stop-codex").addEventListener("click", () => {
  if (!state.connected || state.codexStopped) return;
  state.codexStopped = true;
  $("#codex-status").textContent = "Durduruldu";
  $("#codex-status").className = "muted";
  $("#codex-output").textContent = "Örnek oturum durduruldu. Görev ve karar geçmişi korundu.";
  refreshControls();
  log("Codex örnek oturumu durduruldu; görev tamamlandı sayılmadı.");
});

$("#stop-recipient").addEventListener("click", () => {
  if (!state.connected) return;
  const handoffRun = state.recipientState !== "standalone";
  state.recipientState = "stopped";
  if (handoffRun) state.stage = "stopped";
  $("#recipient-status").textContent = "Durduruldu";
  $("#recipient-status").className = "muted";
  $("#stop-recipient").hidden = true;
  recipientOutput([["Oturum durduruldu (örnek).", "muted"], ["Görev ve önceki teslim olayları korundu."]]);
  renderReceipt();
  refreshControls();
  log(state.recipient + " örnek oturumu durduruldu. Görev tamamlandı sayılmadı.");
});

document.addEventListener("keydown", (event) => {
  if (!(event.ctrlKey || event.metaKey)) return;
  if (event.key.toLowerCase() === "k") {
    event.preventDefault();
    if (!$("dialog[open]")) openPalette();
  } else if (["1", "2", "3"].includes(event.key) && !$("dialog[open]")) {
    event.preventDefault();
    setLayout({1: "balanced", 2: "editor", 3: "agents"}[event.key]);
  }
});

openFile("handoff");
renderReceipt();
refreshControls();
