const instanceId = document.body.dataset.instanceId;
const base = document.body.dataset.basePath;
const transcript = document.querySelector("#transcript");
const prompt = document.querySelector("#prompt");
const composer = document.querySelector("#composer");
const attachmentsNode = document.querySelector("#attachments");
const sendButton = document.querySelector("#send-button");
const stopButton = document.querySelector("#stop-button");
const queuePanel = document.querySelector("#queue-panel");
const queueList = document.querySelector("#queue-list");
const runStatus = document.querySelector("#run-status");
const modelSelect = document.querySelector("#model-select");
const effortSelect = document.querySelector("#effort-select");
const modeSelect = document.querySelector("#mode-select");
const mobileModelSelect = document.querySelector("#mobile-model-select");
const mobileEffortSelect = document.querySelector("#mobile-effort-select");
const mobileModeSelect = document.querySelector("#mobile-mode-select");
const composerPlanToggle = document.querySelector("#composer-plan-toggle");
const permissionsSelect = document.querySelector("#permissions-select");
const newUpdatesButton = document.querySelector("#new-updates");
const attachments = [];
const queuedDrafts = [];
let threadId = document.body.dataset.threadId || "";
let turnId = document.body.dataset.activeTurnId || "";
let runState = turnId ? "running" : "idle";
let queuePaused = false;
let queueThreadId = threadId || "new";
let queuePersistence = Promise.resolve();
let sessionNavigationController;
let navigationState;
let lastSeq = Number(document.body.dataset.baseSeq || 0);
let unseenUpdates = 0;
let steerInFlight = false;
let runStartedAt = turnId ? Date.now() : 0;
let eventFrame;
let vimNormal = false;
const eventQueue = [];

async function rpc(method, params) {
  const response = await fetch(base + "/api/rpc", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ method, params: params || {} }),
  });
  const envelope = await response.json();
  if (!response.ok || envelope.error) {
    throw new Error(envelope.error?.message || envelope.error || "Request failed");
  }
  return envelope.result;
}

async function answerRequest(id, result) {
  const response = await fetch(base + "/api/rpc", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id, result }),
  });
  if (!response.ok) throw new Error("Could not answer Codex");
}

function applyEvent(event) {
  if (event.op === "runtimeChanged") {
    location.reload();
    return;
  }
  if (event.threadId && event.threadId !== threadId) {
    updateThreadBadge(event);
    return;
  }
  const stickToBottom = isCloseToBottom();
  if (event.op === "activityUpsert") {
    upsertActivity(event);
    document.querySelector("#empty-state")?.remove();
    if (stickToBottom) scrollToLatest();
    else showNewUpdates();
  } else if (event.op === "upsert") {
    const template = document.createElement("template");
    template.innerHTML = event.html.trim();
    const node = template.content.firstElementChild;
    if (node.matches("[data-plan-proposal]") && event.turnId) {
      node.dataset.planTurnId = event.turnId;
    }
    const existing = document.getElementById(event.domId);
    if (existing) existing.replaceWith(node);
    else transcript.append(node);
    if (node.matches("[data-plan-proposal]")) applyStoredPlanDecision(node);
    if (node.dataset.clientId) {
      document
        .querySelector(`[data-optimistic-id="${node.dataset.clientId}"]`)
        ?.remove();
      acknowledgeDraft(node.dataset.clientId);
    }
    if (node.classList.contains("prose")) removeAgentLoading();
    else if (runState !== "idle") showAgentLoading();
    document.querySelector("#empty-state")?.remove();
    if (stickToBottom) scrollToLatest();
    else showNewUpdates();
  } else if (event.op === "remove") {
    const node = document.getElementById(event.domId);
    const digest = node?.closest("[data-work-digest]");
    node?.remove();
    if (digest && !digest.querySelector("[data-work-items]")?.children.length) {
      digest.remove();
    } else if (digest) {
      refreshWorkDigest(digest);
    }
  } else if (event.op === "turn") {
    handleTurnEvent(event);
  } else if (event.op === "threadStatus") {
    updateThreadBadge(event);
  } else if (event.op === "threadSettings") {
    syncPermissionMode(permissionModeFromSettings(event));
  } else if (event.op === "goal") {
    renderGoal(event.goal || null);
  } else if (event.op === "skillsChanged") {
    skillCache = undefined;
    if (skillTokenAtCursor()) void renderCommandPalette({ forceReload: true });
  } else if (event.op === "resync") {
    void reloadCurrentSession();
  } else if (event.op === "mcpAuth") {
    const button = [...document.querySelectorAll("[data-mcp-login]")].find(
      (candidate) => candidate.dataset.mcpLogin === event.name,
    );
    if (event.success) {
      if (button) {
        button.textContent = "Connected";
        button.disabled = true;
      }
      setTimeout(() => location.reload(), 600);
    } else {
      if (button) {
        button.textContent = "Retry";
        button.disabled = false;
      }
      showError(event.error || "MCP login failed");
    }
  } else if (event.op === "error" || event.op === "notice") {
    showError(event.message);
  }
}

function upsertActivity(event) {
  let digest = document.getElementById(event.digestId);
  if (!digest) {
    const template = document.createElement("template");
    template.innerHTML = event.digestHtml.trim();
    digest = template.content.firstElementChild;
    transcript.append(digest);
  }
  if (event.turnId === turnId && runState !== "idle") {
    digest.dataset.turnStatus = "inProgress";
  }
  const template = document.createElement("template");
  template.innerHTML = event.html.trim();
  const row = template.content.firstElementChild;
  const existing = document.getElementById(event.domId);
  if (existing) existing.replaceWith(row);
  else digest.querySelector("[data-work-items]")?.append(row);
  refreshWorkDigest(digest);
  if (runState !== "idle") showAgentLoading();
}

function refreshWorkDigest(digest, { settle = false } = {}) {
  const rows = [...digest.querySelectorAll(".activity-row")];
  const activeRows = rows.filter((row) =>
    ["inProgress", "running"].includes(row.dataset.activityStatus),
  );
  const failed = rows.filter((row) => row.dataset.activityStatus === "failed");
  const completed = rows.filter((row) => row.dataset.activityStatus === "completed");
  const active = activeRows.length > 0 || digest.dataset.turnStatus === "inProgress";
  const summary = digest.querySelector("[data-work-summary]");
  if (summary) {
    summary.textContent = activeRows.at(-1)?.dataset.activityTitle ||
      (failed.length
        ? `${rows.length} actions`
        : completed.length
          ? `${completed.length} actions completed`
          : `${rows.length} actions`);
  }
  const failure = digest.querySelector("[data-work-failure]");
  if (failure) {
    failure.hidden = failed.length === 0;
    failure.textContent = `${failed.length} failed`;
  }
  const header = digest.querySelector("summary");
  let live = header?.querySelector("[data-work-live]");
  if (active && !live) {
    live = document.createElement("span");
    live.dataset.workLive = "";
    live.className = "live-rail h-4 w-0.5 shrink-0 rounded-full bg-sky-500";
    header?.querySelector(".activity-caret")?.after(live);
  } else if (!active) {
    live?.remove();
  }
  if (settle) {
    const details = digest.querySelector(":scope > details");
    if (details) details.open = failed.length > 0;
  }
}

function connectEvents() {
  const source = new EventSource(base + "/events?after=" + encodeURIComponent(lastSeq));
  source.onopen = () => setConnected(true);
  source.onerror = () => setConnected(false);
  source.onmessage = (message) => {
    const event = JSON.parse(message.data);
    if (event.seq && event.seq <= lastSeq) return;
    if (event.seq) lastSeq = event.seq;
    if (navigationState && event.threadId === navigationState.threadId) {
      navigationState.events.push(event);
      return;
    }
    eventQueue.push(event);
    if (!eventFrame) {
      eventFrame = requestAnimationFrame(() => {
        eventFrame = undefined;
        eventQueue.splice(0).forEach(applyEvent);
      });
    }
  };
}

function setConnected(connected) {
  document.querySelector("#connection-status").textContent = connected
    ? "Connected"
    : "Reconnecting";
  const dot = document.querySelector("#connection-dot");
  dot.classList.toggle("bg-emerald-500", connected);
  dot.classList.toggle("bg-amber-500", !connected);
}

function isCloseToBottom() {
  return (
    transcript.scrollHeight - transcript.scrollTop - transcript.clientHeight < 120
  );
}

function scrollToLatest() {
  transcript.scrollTop = transcript.scrollHeight;
  unseenUpdates = 0;
  newUpdatesButton.hidden = true;
  markThreadRead(threadId);
}

function showNewUpdates() {
  unseenUpdates += 1;
  newUpdatesButton.textContent =
    unseenUpdates === 1 ? "1 new update" : `${unseenUpdates} new updates`;
  newUpdatesButton.hidden = false;
}

newUpdatesButton?.addEventListener("click", scrollToLatest);
transcript?.addEventListener("scroll", () => {
  if (isCloseToBottom()) scrollToLatest();
}, { passive: true });

function threadLink(id) {
  return [...document.querySelectorAll("[data-session-link]")].find(
    (link) => link.dataset.threadId === id,
  );
}

function updateThreadBadge(event) {
  const link = threadLink(event.threadId);
  if (!link) return;
  const live = link.querySelector("[data-thread-live]");
  const unread = link.querySelector("[data-thread-unread]");
  const status = typeof event.status === "string" ? event.status : event.status?.type;
  if (live && (event.op === "threadStatus" || event.op === "turn")) {
    live.classList.toggle("hidden", status !== "active" && status !== "inProgress");
    live.classList.toggle("bg-amber-500", status === "waitingOnApproval" || status === "waitingOnUserInput");
  }
  if (event.threadId !== threadId && event.seq) {
    const read = Number(localStorage.getItem(`codex-web-read:${event.threadId}`) || 0);
    const count = Math.max(1, Number(unread?.textContent || 0) + (event.seq > read ? 1 : 0));
    if (unread) {
      unread.textContent = String(count);
      unread.classList.remove("hidden");
    }
  }
}

function markThreadRead(id) {
  if (!id) return;
  localStorage.setItem(`codex-web-read:${id}`, String(lastSeq));
  const unread = threadLink(id)?.querySelector("[data-thread-unread]");
  unread?.classList.add("hidden");
  if (unread) unread.textContent = "0";
}

function setRunState(state) {
  const stickToBottom = isCloseToBottom();
  runState = state;
  const active = state === "sending" || state === "running" || state === "stopping";
  const sending = state === "sending";
  const stopping = state === "stopping";
  runStatus.hidden = !active;
  document.querySelector("#run-status-label").textContent =
    state === "sending"
      ? "Starting Codex…"
      : state === "stopping"
        ? "Stopping execution…"
        : "Codex is working…";
  sendButton.disabled = sending || stopping;
  document.querySelector("#send-spinner").classList.toggle("hidden", !sending);
  document.querySelector("#send-icon").classList.toggle("hidden", sending);
  sendButton.title = state === "running" ? "Queue message" : "Send message";
  sendButton.ariaLabel = sendButton.title;
  stopButton.hidden = !turnId && state !== "running" && !stopping;
  stopButton.disabled = stopping;
  document.querySelector("#stop-spinner").classList.toggle("hidden", !stopping);
  document.querySelector("#stop-icon").classList.toggle("hidden", stopping);
  stopButton.title = stopping ? "Stopping execution" : "Stop execution";
  stopButton.ariaLabel = stopButton.title;
  document.querySelector("#composer-hint").textContent =
    state === "running"
      ? "Send to queue · Shift + Enter for a new line"
      : "Shift + Enter for a new line";
  if (active) showAgentLoading();
  else removeAgentLoading();
  if (stickToBottom) transcript.scrollTop = transcript.scrollHeight;
  renderQueue();
  refreshDocumentTitle();
  refreshStatusline();
}

setInterval(() => {
  const elapsed = document.querySelector("#run-elapsed");
  if (!elapsed) return;
  if (!runStartedAt || runState === "idle") {
    elapsed.textContent = "";
    return;
  }
  const seconds = Math.max(0, Math.floor((Date.now() - runStartedAt) / 1000));
  elapsed.textContent = `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}, 1000);

function showAgentLoading() {
  let loading = document.querySelector("#agent-loading");
  if (!loading) {
    loading = document.createElement("article");
    loading.id = "agent-loading";
    loading.className = "mb-7 flex items-start";
    loading.innerHTML =
      '<div class="flex items-center gap-3 rounded-xl border border-border bg-foreground/[0.025] px-3.5 py-3 text-sm"><span class="grid size-7 shrink-0 place-items-center rounded-lg bg-foreground/[0.06] font-mono text-[0.65rem] font-semibold">&gt;_</span><span class="min-w-0"><strong class="block text-xs font-medium text-foreground">Codex is working</strong><span class="mt-1 flex items-center gap-1.5 text-xs text-muted-foreground"><span class="flex gap-1" aria-hidden="true"><span class="size-1 animate-pulse rounded-full bg-current [animation-delay:-0.3s]"></span><span class="size-1 animate-pulse rounded-full bg-current [animation-delay:-0.15s]"></span><span class="size-1 animate-pulse rounded-full bg-current"></span></span><span>You can keep typing — new messages will queue.</span></span></span></div>';
  }
  transcript.append(loading);
}

function removeAgentLoading() {
  document.querySelector("#agent-loading")?.remove();
}

function handleTurnEvent(event) {
  if (event.status === "inProgress") {
    deactivatePlanProposals();
    turnId = event.turnId;
    runStartedAt = Date.now();
    setRunState("running");
    return;
  }
  if (turnId && event.turnId && event.turnId !== turnId) return;
  const digest = document.getElementById(`work-${event.turnId}`);
  if (digest) {
    digest.dataset.turnStatus = event.status;
    refreshWorkDigest(digest, { settle: true });
  }
  const stopped = runState === "stopping";
  turnId = "";
  steerInFlight = false;
  runStartedAt = 0;
  setRunState("idle");
  activateLatestPlanProposal(event.turnId);
  if (!stopped && !queuePaused) sendNextQueued();
}

async function ensureThread(overrides = {}) {
  if (threadId) return threadId;
  const result = await rpc("thread/start", {
    ephemeral: false,
    cwd: document.body.dataset.workspaceCwd,
    ...overrides,
  });
  threadId = result.thread.id;
  const previousQueueKey = queueSessionKey(queueThreadId);
  queueThreadId = threadId;
  sessionStorage.removeItem(previousQueueKey);
  void persistQueue();
  document.body.dataset.threadId = threadId;
  composer.dataset.threadId = threadId;
  localStorage.setItem(`codex-web-mode:${threadId}`, modeSelect?.value || "default");
  history.replaceState({}, "", base + "/thread/" + encodeURIComponent(threadId));
  return threadId;
}

function permissionModeFromSettings(settings) {
  const profile = settings.activePermissionProfile?.id;
  if (profile === ":danger-full-access") return "full-access";
  if (profile === ":read-only") return "read-only";
  if (profile === ":workspace") return "workspace";
  const sandbox = settings.sandboxPolicy?.type || settings.sandbox?.type;
  if (settings.approvalPolicy === "never" && sandbox === "danger-full-access") {
    return "full-access";
  }
  if (sandbox === "read-only") return "read-only";
  return "workspace";
}

function permissionSettings(mode) {
  return {
    workspace: { permissions: ":workspace", approvalPolicy: "on-request" },
    "full-access": { permissions: ":danger-full-access", approvalPolicy: "never" },
    "read-only": { permissions: ":read-only", approvalPolicy: "on-request" },
  }[mode] || { permissions: ":workspace", approvalPolicy: "on-request" };
}

function syncPermissionMode(mode) {
  if (!permissionsSelect) return;
  permissionsSelect.value = mode;
  permissionsSelect.dataset.confirmedValue = mode;
  permissionsSelect.disabled = false;
}

async function updatePermissionMode(nextMode) {
  if (!permissionsSelect) return;
  const previousMode = permissionsSelect.dataset.confirmedValue || "workspace";
  if (
    nextMode === "full-access" &&
    !window.confirm(
      "Enable YOLO mode? Codex will have unrestricted filesystem and network access and will not ask for command approval.",
    )
  ) {
    permissionsSelect.value = previousMode;
    return;
  }
  const settings = permissionSettings(nextMode);
  permissionsSelect.disabled = true;
  try {
    const hadThread = Boolean(threadId);
    const activeThreadId = await ensureThread(settings);
    if (hadThread) {
      await rpc("thread/settings/update", { threadId: activeThreadId, ...settings });
    }
    syncPermissionMode(nextMode);
  } catch (error) {
    syncPermissionMode(previousMode);
    showError(error.message);
  }
}

if (permissionsSelect) {
  permissionsSelect.dataset.confirmedValue = permissionsSelect.value;
  permissionsSelect.addEventListener("change", () => {
    void updatePermissionMode(permissionsSelect.value);
  });
}

function queueSessionKey(id = queueThreadId) {
  return `codex-web-queue:${instanceId}:${id || "new"}`;
}

function draftStorageKey(draft) {
  return `${instanceId}:${draft.clientUserMessageId}`;
}

function openDraftDatabase() {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open("codex-web", 1);
    request.onupgradeneeded = () => request.result.createObjectStore("drafts");
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

async function withDraftStore(mode, operation) {
  const database = await openDraftDatabase();
  return new Promise((resolve, reject) => {
    const transaction = database.transaction("drafts", mode);
    const store = transaction.objectStore("drafts");
    operation(store, resolve, reject);
    transaction.onerror = () => reject(transaction.error);
    transaction.oncomplete = () => database.close();
  });
}

function persistQueue() {
  const state = {
    paused: queuePaused,
    ids: queuedDrafts.map((draft) => draft.clientUserMessageId),
  };
  sessionStorage.setItem(queueSessionKey(), JSON.stringify(state));
  const drafts = queuedDrafts.map((draft) => ({
    text: draft.text,
    status: draft.status,
    clientUserMessageId: draft.clientUserMessageId,
    images: draft.images.map((image) => ({ ...image })),
  }));
  queuePersistence = queuePersistence.then(() => Promise.all(drafts.map(async (draft) => {
    const images = await Promise.all(draft.images.map(async (image) => ({
      name: image.name || "pasted-image",
      type: image.type || "image/png",
      blob: await fetch(image.url).then((response) => response.blob()),
    })));
    const record = {
      text: draft.text,
      status: draft.status || "queued",
      clientUserMessageId: draft.clientUserMessageId,
      images,
      savedAt: Date.now(),
    };
    await withDraftStore("readwrite", (store, resolve, reject) => {
      const request = store.put(record, draftStorageKey(draft));
      request.onsuccess = () => resolve();
      request.onerror = () => reject(request.error);
    });
  }))).catch((error) => console.warn("Could not persist queued messages", error));
  return queuePersistence;
}

function deletePersistedDraft(draft) {
  queuePersistence = queuePersistence.then(() => withDraftStore("readwrite", (store, resolve, reject) => {
    const request = store.delete(draftStorageKey(draft));
    request.onsuccess = () => resolve();
    request.onerror = () => reject(request.error);
  })).catch((error) => console.warn("Could not remove queued message", error));
  return queuePersistence;
}

function blobUrl(blob) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result);
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(blob);
  });
}

async function restoreQueue(id) {
  queueThreadId = id || "new";
  queuedDrafts.splice(0);
  const state = JSON.parse(sessionStorage.getItem(queueSessionKey()) || "null");
  queuePaused = Boolean(state?.paused);
  for (const clientUserMessageId of state?.ids || []) {
    const record = await withDraftStore("readonly", (store, resolve, reject) => {
      const request = store.get(`${instanceId}:${clientUserMessageId}`);
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });
    if (!record) continue;
    const images = await Promise.all(record.images.map(async (image) => ({
      name: image.name,
      type: image.type,
      url: await blobUrl(image.blob),
    })));
    const status = !turnId && record.status !== "queued" ? "queued" : record.status;
    queuedDrafts.push(makeDraft(record.text, images, clientUserMessageId, status));
  }
  renderQueue();
}

function makeDraft(text, images, clientUserMessageId, status = "queued") {
  const input = [];
  if (text) input.push({ type: "text", text, text_elements: [] });
  input.push(...images.map((image) => ({ type: "image", url: image.url })));
  return { text, images, input, clientUserMessageId, status };
}

function acknowledgeDraft(clientId) {
  const index = queuedDrafts.findIndex(
    (draft) => draft.clientUserMessageId === clientId,
  );
  if (index < 0) return;
  const [draft] = queuedDrafts.splice(index, 1);
  void deletePersistedDraft(draft);
  void persistQueue();
  renderQueue();
}

function addOptimisticMessage(draft) {
  const article = document.createElement("article");
  article.className = "mb-7 flex justify-end";
  article.dataset.optimisticId = draft.clientUserMessageId;
  const content = document.createElement("div");
  content.className =
    "message-content min-w-0 max-w-[82%] rounded-xl bg-foreground/[0.06] px-4 py-2.5 text-sm whitespace-pre-wrap";
  content.textContent = draft.text;
  for (const input of draft.images) {
    const image = document.createElement("img");
    image.className = "mt-2 max-h-80 max-w-full rounded-lg object-contain";
    image.src = input.url;
    image.alt = "Attached image";
    content.append(image);
  }
  article.append(content);
  transcript.append(article);
  document.querySelector("#empty-state")?.remove();
  transcript.scrollTop = transcript.scrollHeight;
  return article;
}

function takeDraft() {
  const text = prompt.value.trim();
  if (!text && attachments.length === 0) return null;
  const images = attachments.map((attachment) => ({ ...attachment }));
  const clientUserMessageId =
    globalThis.crypto?.randomUUID?.() ||
    `web-${Date.now()}-${Math.random().toString(36).slice(2)}`;
  prompt.value = "";
  attachments.splice(0);
  renderAttachments();
  resizePrompt();
  return makeDraft(text, images, clientUserMessageId);
}

async function sendDraft(draft, steer = false) {
  draft.status = steer ? "steering" : "sending";
  void persistQueue();
  setRunState("sending");
  let optimisticMessage;
  try {
    await ensureThread();
    optimisticMessage = addOptimisticMessage(draft);
    if (steer) {
      const result = await rpc("turn/steer", {
        threadId,
        expectedTurnId: turnId,
        clientUserMessageId: draft.clientUserMessageId,
        input: draft.input,
      });
      turnId = result.turnId || turnId;
      steerInFlight = true;
    } else {
      const result = await rpc("turn/start", {
        threadId,
        clientUserMessageId: draft.clientUserMessageId,
        input: draft.input,
        ...currentThreadSettings(),
      });
      turnId = result.turn.id;
      runStartedAt = Date.now();
    }
    draft.status = "awaiting";
    void persistQueue();
    queuePaused = false;
    setRunState("running");
    return true;
  } catch (error) {
    optimisticMessage?.remove();
    draft.status = "queued";
    steerInFlight = false;
    queuePaused = !steer;
    void persistQueue();
    setRunState(steer && turnId ? "running" : "idle");
    showError(error.message);
    return false;
  }
}

function enqueueDraft(draft) {
  queuedDrafts.push(draft);
  void persistQueue();
  renderQueue();
}

function sendNextQueued() {
  if (runState !== "idle" || turnId) return;
  const draft = queuedDrafts.find((candidate) => candidate.status === "queued");
  if (!draft) return;
  renderQueue();
  void sendDraft(draft);
}

function renderQueue() {
  const hasQueued = queuedDrafts.length > 0;
  queuePanel.hidden = !hasQueued;
  document.querySelector("#queue-count").textContent = `${queuedDrafts.length} queued`;
  document.querySelector("#queue-state").textContent = queuePaused ? "Paused" : "";
  document.querySelector("#run-queue-button").hidden = runState !== "idle";
  queueList.replaceChildren();
  queuedDrafts.forEach((draft, index) => {
    const row = document.createElement("div");
    row.className = "flex items-center gap-2 rounded-lg px-2 py-2 hover:bg-foreground/[0.035]";
    const order = document.createElement("span");
    order.className = "text-[0.7rem] font-medium text-muted-foreground";
    order.textContent = draft.status === "queued" ? (index === 0 ? "NEXT" : String(index + 1)) : draft.status.toUpperCase();
    const summary = document.createElement("span");
    summary.className = "min-w-0 flex-1 truncate text-xs";
    summary.textContent = draft.text || `${draft.images.length} image attachment`;
    const remove = document.createElement("button");
    remove.type = "button";
    remove.className = "rounded-md px-2 py-1 text-xs text-muted-foreground hover:bg-foreground/5 hover:text-foreground";
    remove.textContent = "Remove";
    remove.dataset.queueIndex = String(index);
    const up = document.createElement("button");
    up.type = "button";
    up.className = "rounded px-1 text-xs text-muted-foreground hover:text-foreground";
    up.textContent = "↑";
    up.dataset.queueUp = String(index);
    up.hidden = index === 0 || draft.status !== "queued";
    const down = document.createElement("button");
    down.type = "button";
    down.className = up.className;
    down.textContent = "↓";
    down.dataset.queueDown = String(index);
    down.hidden = index === queuedDrafts.length - 1 || draft.status !== "queued";
    remove.hidden = draft.status !== "queued";
    row.append(order, summary, up, down, remove);
    queueList.append(row);
  });
}

const commands = [
  ["new", "Start a new conversation"],
  ["clear", "Clear the view and start a new conversation"],
  ["resume", "Find another conversation"],
  ["fork", "Fork this conversation"],
  ["rename", "Rename this conversation"],
  ["archive", "Archive this conversation"],
  ["delete", "Delete this conversation"],
  ["compact", "Compact conversation context"],
  ["init", "Create project instructions"],
  ["review", "Review uncommitted changes"],
  ["plan", "Switch to plan mode"],
  ["goal", "Set the session goal"],
  ["settings", "Open account, MCP, and appearance settings"],
  ["side", "Fork an ephemeral side conversation"],
  ["btw", "Ask in an ephemeral side conversation"],
  ["model", "Choose model and effort"],
  ["permissions", "Inspect permission settings"],
  ["approve", "Inspect approval settings"],
  ["personality", "Set the response personality"],
  ["experimental", "Show experimental features"],
  ["memories", "Inspect memory settings"],
  ["status", "Show session status"],
  ["usage", "Show account usage"],
  ["skills", "List available skills"],
  ["hooks", "List lifecycle hooks"],
  ["mcp", "Show MCP servers"],
  ["apps", "Show connected apps"],
  ["plugins", "Show installed plugins"],
  ["import", "Find settings and chats to import"],
  ["feedback", "Send feedback with optional logs"],
  ["agent", "Show loaded agent sessions"],
  ["subagents", "Show loaded subagent sessions"],
  ["ps", "Show background terminals"],
  ["clean", "Stop background terminals"],
  ["copy", "Copy the latest response"],
  ["mention", "Mention a workspace file"],
  ["raw", "Toggle copy-friendly transcript"],
  ["theme", "Switch light, dark, or system theme"],
  ["title", "Toggle detailed browser titles"],
  ["statusline", "Toggle the compact session status line"],
  ["keymap", "Show browser keyboard shortcuts"],
  ["vim", "Toggle Vim-style composer mode"],
  ["pets", "Toggle the execution companion"],
  ["stop", "Stop the current turn"],
  ["logout", "Log out of Codex"],
  ["quit", "Stop this Codex Web process"],
  ["exit", "Stop this Codex Web process"],
];
let skillCache;
let skillCacheCwd;
let skillRequest;
let skillLoadErrors = [];
let pendingSuggestionAccept = false;
let suggestionState = { items: [], selected: 0, token: null };

function availableCommands() {
  const needsThread = new Set(["fork", "rename", "archive", "delete", "compact", "review", "goal", "side", "btw", "ps", "clean"]);
  return commands.filter(([name]) => threadId || !needsThread.has(name));
}

function skillTokenAtCursor() {
  const cursor = prompt.selectionStart;
  const match = prompt.value.slice(0, cursor).match(/(^|\s)(\$[A-Za-z0-9_:-]*)$/);
  if (!match) return null;
  return { start: cursor - match[2].length, end: cursor, query: match[2].slice(1).toLowerCase() };
}

async function availableSkills(forceReload = false) {
  const cwd = document.body.dataset.workspaceCwd;
  if (forceReload) {
    skillCache = undefined;
    skillCacheCwd = undefined;
  }
  if (skillCache && skillCacheCwd === cwd) return skillCache;
  if (!skillRequest) {
    skillRequest = rpc("skills/list", {
      cwds: [cwd],
      forceReload,
    }).then((result) => {
      skillLoadErrors = (result.data || []).flatMap((entry) => entry.errors || []);
      skillCache = (result.data || [])
        .flatMap((entry) => entry.skills || [])
        .filter((skill) => skill.enabled !== false)
        .filter((skill, index, skills) => skills.findIndex((item) => item.name === skill.name) === index);
      skillCacheCwd = cwd;
      return skillCache;
    }).finally(() => { skillRequest = null; });
  }
  return skillRequest;
}

function suggestionButton(item, index) {
  const button = document.createElement("button");
  button.type = "button";
  button.role = "option";
  button.ariaSelected = index === suggestionState.selected ? "true" : "false";
  button.className = "flex w-full min-w-0 items-start gap-3 rounded-lg px-3 py-2 text-left hover:bg-foreground/5 aria-selected:bg-foreground/[0.07]";
  const name = document.createElement("span");
  name.className = "w-28 shrink-0 truncate font-mono text-xs";
  name.textContent = item.label;
  const description = document.createElement("span");
  description.className = "min-w-0 flex-1 text-xs text-muted-foreground [overflow-wrap:anywhere]";
  description.textContent = item.description;
  button.append(name, description);
  button.addEventListener("pointerdown", (event) => {
    event.preventDefault();
    selectSuggestion(index);
  });
  return button;
}

function paintSuggestions(items, token = null) {
  const palette = document.querySelector("#command-palette");
  if (!palette) return;
  palette.classList.remove("text-xs", "text-destructive");
  suggestionState = { items, selected: 0, token };
  palette.replaceChildren(...items.map(suggestionButton));
  palette.hidden = items.length === 0;
}

async function renderCommandPalette({ forceReload = false } = {}) {
  const palette = document.querySelector("#command-palette");
  if (!palette) return;
  const value = prompt.value;
  if (value.startsWith("/") && !value.includes(" ") && !value.includes("\n")) {
    pendingSuggestionAccept = false;
    const query = value.slice(1).toLowerCase();
    paintSuggestions(availableCommands()
      .filter(([name]) => name.startsWith(query))
      .map(([name, description]) => ({ kind: "command", label: `/${name}`, value: `/${name}`, description })));
    return;
  }
  const token = skillTokenAtCursor();
  if (!token) {
    pendingSuggestionAccept = false;
    palette.hidden = true;
    suggestionState = { items: [], selected: 0, token: null };
    return;
  }
  suggestionState = { items: [], selected: 0, token };
  palette.classList.remove("text-xs", "text-destructive");
  palette.hidden = false;
  palette.innerHTML = '<p class="px-3 py-2 text-xs text-muted-foreground">Loading skills…</p>';
  try {
    const skills = await availableSkills(forceReload);
    if (skillTokenAtCursor()?.start !== token.start || skillTokenAtCursor()?.query !== token.query) {
      pendingSuggestionAccept = false;
      return;
    }
    const matches = skills
      .filter((skill) => skill.name.toLowerCase().includes(token.query))
      .sort((left, right) => {
        const leftPrefix = left.name.toLowerCase().startsWith(token.query);
        const rightPrefix = right.name.toLowerCase().startsWith(token.query);
        return Number(rightPrefix) - Number(leftPrefix) || left.name.localeCompare(right.name);
      })
      .map((skill) => ({
        kind: "skill",
        label: `$${skill.name}`,
        value: `$${skill.name} `,
        description: skill.interface?.shortDescription || skill.shortDescription || skill.description || "Codex skill",
      }));
    paintSuggestions(matches, token);
    if (pendingSuggestionAccept && matches.length) {
      pendingSuggestionAccept = false;
      selectSuggestion();
      return;
    }
    if (!suggestionState.items.length) {
      pendingSuggestionAccept = false;
      palette.hidden = false;
      palette.innerHTML = `<p class="px-3 py-2 text-xs text-muted-foreground">${
        skills.length
          ? "No matching skills"
          : skillLoadErrors.length
            ? "Skills could not be loaded for this workspace"
            : "No skills are available for this workspace"
      }</p>`;
    }
  } catch (error) {
    pendingSuggestionAccept = false;
    palette.hidden = false;
    palette.textContent = `Could not load skills: ${error.message}`;
    palette.classList.add("text-xs", "text-destructive");
  }
}

function selectSuggestion(index = suggestionState.selected) {
  const item = suggestionState.items[index];
  if (!item) return false;
  if (item.kind === "command") {
    prompt.value = item.value;
    prompt.setSelectionRange(prompt.value.length, prompt.value.length);
  } else {
    const token = suggestionState.token;
    prompt.setRangeText(item.value, token.start, token.end, "end");
  }
  document.querySelector("#command-palette").hidden = true;
  suggestionState = { items: [], selected: 0, token: null };
  prompt.focus();
  resizePrompt();
  return true;
}

function moveSuggestion(delta) {
  if (!suggestionState.items.length) return false;
  suggestionState.selected = (suggestionState.selected + delta + suggestionState.items.length) % suggestionState.items.length;
  const palette = document.querySelector("#command-palette");
  [...palette.querySelectorAll('[role="option"]')].forEach((option, index) => {
    option.ariaSelected = index === suggestionState.selected ? "true" : "false";
  });
  palette.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
  return true;
}

function appendSystemCard(title, value) {
  const article = document.createElement("article");
  article.className = "mb-4 min-w-0 rounded-lg border border-border bg-foreground/[0.018] p-3 [overflow-wrap:anywhere]";
  article.dataset.systemCard = "";
  const heading = document.createElement("strong");
  heading.className = "text-sm";
  heading.textContent = title;
  const content = document.createElement("pre");
  content.className = "mt-2 max-h-80 overflow-auto whitespace-pre-wrap font-mono text-xs text-muted-foreground";
  content.textContent = typeof value === "string" ? value : JSON.stringify(value, null, 2);
  article.append(heading, content);
  transcript.append(article);
  scrollToLatest();
}

async function executeCommand(source) {
  const [name, ...arguments_] = source.slice(1).trim().split(/\s+/);
  const argument = arguments_.join(" ");
  if (![...availableCommands()].some(([candidate]) => candidate === name)) return false;
  prompt.value = "";
  renderCommandPalette();
  try {
    if (name === "new" || name === "clear") return void document.querySelector("[data-thread-id='']")?.click();
    if (name === "resume") return void document.querySelector("#thread-search")?.focus();
    if (name === "model") return void modelSelect?.focus();
    if (name === "mention") {
      prompt.value = "@";
      prompt.focus();
      return true;
    }
    if (name === "plan") {
      modeSelect.value = "plan";
      modeSelect.dispatchEvent(new Event("change"));
      return true;
    }
    if (name === "copy") {
      const latest = [...transcript.querySelectorAll(".prose")].at(-1)?.innerText || "";
      await navigator.clipboard.writeText(latest);
      return true;
    }
    if (name === "raw") {
      transcript.classList.toggle("raw-transcript");
      return true;
    }
    if (name === "theme") {
      const current = localStorage.getItem("codex-web-theme") || "dark";
      const next = argument || ({ system: "light", light: "dark", dark: "system" }[current]);
      setTheme(next);
      return true;
    }
    if (name === "title") {
      const enabled = localStorage.getItem("codex-web-detailed-title") !== "true";
      localStorage.setItem("codex-web-detailed-title", String(enabled));
      refreshDocumentTitle();
      return true;
    }
    if (name === "statusline") {
      const statusline = document.querySelector("#web-statusline");
      const enabled = statusline.hidden;
      statusline.hidden = !enabled;
      localStorage.setItem("codex-web-statusline", String(enabled));
      refreshStatusline();
      return true;
    }
    if (name === "keymap") {
      appendSystemCard("Keyboard shortcuts", "Enter  Send\nShift+Enter  New line\nCtrl/Cmd+K  Commands\nEscape  Close commands or stop\nCtrl/Cmd+Shift+V  Paste image");
      return true;
    }
    if (name === "vim") {
      const enabled = document.body.dataset.vim !== "true";
      document.body.dataset.vim = String(enabled);
      localStorage.setItem("codex-web-vim", String(enabled));
      appendSystemCard("Composer", enabled ? "Vim navigation enabled. Escape enters normal mode; i returns to insert mode." : "Vim navigation disabled.");
      return true;
    }
    if (name === "pets") {
      const pet = document.querySelector("#codex-pet");
      const enabled = pet.classList.contains("hidden");
      pet.classList.toggle("hidden", !enabled);
      localStorage.setItem("codex-web-pets", String(enabled));
      return true;
    }
    if (name === "stop") return void stopButton?.click();
    if (name === "quit" || name === "exit") {
      if (confirm("Stop this Codex Web process?")) {
        await fetch(base + "/api/shutdown", { method: "POST" });
        appendSystemCard("Codex Web", "The process is stopping.");
      }
      return true;
    }
    if (name === "status") {
      appendSystemCard("Session status", { threadId, turnId, runState, model: modelSelect?.value, effort: effortSelect?.value, mode: modeSelect?.value, queued: queuedDrafts.length });
      return true;
    }
    if (name === "rename") {
      const nextName = argument || window.prompt("Conversation name");
      if (nextName) await rpc("thread/name/set", { threadId, name: nextName });
      return true;
    }
    if (name === "goal") {
      openGoalModal();
      if (argument) document.querySelector("#goal-objective").value = argument;
      return true;
    }
    if (name === "settings") {
      openModal("settings-modal");
      return true;
    }
    if (name === "personality") {
      const personality = argument || window.prompt("Personality (friendly, pragmatic, or none)");
      if (personality) await rpc("thread/settings/update", { threadId, personality: personality === "none" ? null : personality });
      return true;
    }
    if (name === "fork") {
      const result = await rpc("thread/fork", { threadId });
      const id = result.thread?.id;
      if (id) location.assign(`${base}/thread/${encodeURIComponent(id)}`);
      return true;
    }
    if (name === "side" || name === "btw") {
      const result = await rpc("thread/fork", { threadId, ephemeral: true, excludeTurns: true });
      const sideId = result.thread?.id;
      if (!sideId) throw new Error("Codex could not create the side conversation");
      if (name === "btw") {
        const text = argument || window.prompt("Ask in the side conversation");
        if (text) await rpc("turn/start", { threadId: sideId, input: [{ type: "text", text, text_elements: [] }] });
      }
      location.assign(`${base}/thread/${encodeURIComponent(sideId)}`);
      return true;
    }
    if (name === "init") {
      const draft = makeDraft(
        "Create an AGENTS.md file with concise project instructions for future coding agents. Inspect the repository first, preserve existing instructions, and include only commands and conventions that are supported by the project.",
        [],
        globalThis.crypto?.randomUUID?.() || `web-${Date.now()}`,
      );
      enqueueDraft(draft);
      if (!turnId) sendNextQueued();
      return true;
    }
    if (name === "compact") {
      await rpc("thread/compact/start", { threadId });
      return true;
    }
    if (name === "review") {
      const result = await rpc("review/start", { threadId, target: { type: "uncommittedChanges" }, delivery: "inline" });
      turnId = result.turn?.id || turnId;
      setRunState("running");
      return true;
    }
    if (name === "archive" || name === "delete") {
      if (!confirm(`${name === "delete" ? "Delete" : "Archive"} this conversation?`)) return true;
      await rpc(`thread/${name}`, { threadId });
      location.assign(base);
      return true;
    }
    if (name === "logout") {
      if (confirm("Log out of Codex?")) await rpc("account/logout", {});
      return true;
    }
    const listMethods = {
      usage: ["account/usage/read", {}],
      skills: ["skills/list", { cwds: [document.body.dataset.workspaceCwd], forceReload: false }],
      hooks: ["hooks/list", {}],
      mcp: ["mcpServerStatus/list", { limit: 100, detail: "toolsAndAuthOnly" }],
      apps: ["app/list", { limit: 100 }],
      plugins: ["plugin/list", {}],
      permissions: ["config/read", { includeLayers: true }],
      approve: ["config/read", { includeLayers: true }],
      memories: ["config/read", { includeLayers: true }],
      experimental: ["experimentalFeature/list", {}],
      import: ["externalAgentConfig/detect", { cwds: [document.body.dataset.workspaceCwd] }],
      agent: ["thread/loaded/list", {}],
      subagents: ["thread/loaded/list", {}],
      ps: ["thread/backgroundTerminals/list", { threadId, limit: 100 }],
    };
    if (name === "feedback") {
      const reason = argument || window.prompt("What should Codex improve?");
      if (reason) await rpc("feedback/upload", { classification: "bug", reason, threadId: threadId || null, includeLogs: confirm("Include Codex logs with this feedback?") });
      return true;
    }
    if (name === "clean") {
      await rpc("thread/backgroundTerminals/clean", { threadId });
      return true;
    }
    if (listMethods[name]) {
      appendSystemCard(`/${name}`, await rpc(...listMethods[name]));
      return true;
    }
  } catch (error) {
    showError(error.message);
  }
  return true;
}

const { setTheme, openModal, closeModal } = globalThis.codexWebSettings;
document.querySelectorAll("#goal-modal").forEach((modal) => {
  modal.addEventListener("click", (event) => {
    if (event.target === modal) closeModal(modal.id);
  });
});

function renderGoal(goal) {
  const objective = document.querySelector("#goal-objective");
  const status = document.querySelector("#goal-status");
  const budget = document.querySelector("#goal-token-budget");
  const usage = document.querySelector("#goal-usage");
  if (!objective || !status || !budget || !usage) return;
  objective.value = goal?.objective || "";
  status.value = goal?.status || "active";
  budget.value = goal?.tokenBudget || "";
  usage.classList.toggle("hidden", !goal);
  usage.classList.toggle("grid", Boolean(goal));
  usage.replaceChildren();
  if (goal) {
    const tokens = document.createElement("span");
    tokens.innerHTML = `<strong class="block text-foreground">${Number(goal.tokensUsed || 0).toLocaleString()}</strong>tokens used`;
    const elapsed = document.createElement("span");
    const seconds = Number(goal.timeUsedSeconds || 0);
    elapsed.innerHTML = `<strong class="block text-foreground">${seconds < 60 ? seconds + "s" : Math.floor(seconds / 60) + "m"}</strong>elapsed`;
    usage.append(tokens, elapsed);
  }
}

async function openGoalModal() {
  if (!threadId) return showError("Start or resume a conversation before setting a goal");
  openModal("goal-modal");
  const error = document.querySelector("#goal-error");
  error.classList.add("hidden");
  try {
    renderGoal((await rpc("thread/goal/get", { threadId })).goal);
  } catch (cause) {
    error.textContent = cause.message;
    error.classList.remove("hidden");
  }
}

document.querySelector("#goal-button")?.addEventListener("click", openGoalModal);
document.querySelector("#goal-form")?.addEventListener("submit", async (event) => {
  event.preventDefault();
  const error = document.querySelector("#goal-error");
  try {
    const rawBudget = document.querySelector("#goal-token-budget").value;
    const result = await rpc("thread/goal/set", {
      threadId,
      objective: document.querySelector("#goal-objective").value.trim(),
      status: document.querySelector("#goal-status").value,
      tokenBudget: rawBudget ? Number(rawBudget) : null,
    });
    renderGoal(result.goal);
    closeModal("goal-modal");
  } catch (cause) {
    error.textContent = cause.message;
    error.classList.remove("hidden");
  }
});
document.querySelector("#goal-clear")?.addEventListener("click", async () => {
  if (!threadId || !confirm("Clear this session goal?")) return;
  await rpc("thread/goal/clear", { threadId });
  renderGoal(null);
  closeModal("goal-modal");
});

function refreshDocumentTitle() {
  const title = document.querySelector("#thread-title")?.textContent || "New task";
  const detailed = localStorage.getItem("codex-web-detailed-title") === "true";
  document.title = detailed && runState !== "idle" ? `● ${title} · Codex` : `${title} - Codex`;
}

function refreshStatusline() {
  const statusline = document.querySelector("#web-statusline");
  if (!statusline || statusline.hidden) return;
  statusline.textContent = [modelSelect?.value, effortSelect?.value, modeSelect?.value].filter(Boolean).join(" · ");
}

const statusline = document.querySelector("#web-statusline");
if (statusline) statusline.hidden = localStorage.getItem("codex-web-statusline") !== "true";
document.body.dataset.vim = localStorage.getItem("codex-web-vim") === "true" ? "true" : "false";
vimNormal = document.body.dataset.vim === "true";
document.querySelector("#codex-pet")?.classList.toggle("hidden", localStorage.getItem("codex-web-pets") !== "true");
refreshStatusline();

composer?.addEventListener("submit", async (event) => {
  event.preventDefault();
  if (prompt.value.trim().startsWith("/") && await executeCommand(prompt.value.trim())) return;
  const draft = takeDraft();
  if (!draft) return;
  if (runState !== "idle" || turnId) {
    enqueueDraft(draft);
    if (!steerInFlight && turnId && runState === "running") {
      void sendDraft(draft, true);
    }
  } else if (queuedDrafts.length > 0) {
    enqueueDraft(draft);
    queuePaused = false;
    sendNextQueued();
  } else {
    void sendDraft(draft);
  }
});

prompt?.addEventListener("keydown", (event) => {
  if (document.body.dataset.vim === "true") {
    if (event.key === "Escape") {
      event.preventDefault();
      vimNormal = true;
      document.querySelector("#composer-hint").textContent = "Vim · NORMAL";
      return;
    }
    if (vimNormal) {
      const cursor = prompt.selectionStart;
      if (event.key === "i") {
        event.preventDefault();
        vimNormal = false;
        document.querySelector("#composer-hint").textContent = "Vim · INSERT";
      } else if (event.key === "h" || event.key === "l") {
        event.preventDefault();
        const next = Math.max(0, Math.min(prompt.value.length, cursor + (event.key === "h" ? -1 : 1)));
        prompt.setSelectionRange(next, next);
      } else if (event.key === "0" || event.key === "$") {
        event.preventDefault();
        const next = event.key === "0" ? 0 : prompt.value.length;
        prompt.setSelectionRange(next, next);
      } else if (event.key === "x") {
        event.preventDefault();
        prompt.setRangeText("", cursor, cursor + 1, "start");
      } else {
        event.preventDefault();
      }
      return;
    }
  }
  if (!document.querySelector("#command-palette")?.hidden) {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      moveSuggestion(event.key === "ArrowDown" ? 1 : -1);
      return;
    }
    if ((event.key === "Tab" || (event.key === "Enter" && !event.shiftKey)) && suggestionState.items.length) {
      event.preventDefault();
      selectSuggestion();
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      pendingSuggestionAccept = false;
      document.querySelector("#command-palette").hidden = true;
      return;
    }
  }
  if (event.key === "Tab" && skillTokenAtCursor()) {
    event.preventDefault();
    pendingSuggestionAccept = true;
    void renderCommandPalette();
    return;
  }
  if (event.key === "Enter" && !event.shiftKey) {
    event.preventDefault();
    composer.requestSubmit();
  }
});
prompt?.addEventListener("input", () => {
  resizePrompt();
  void renderCommandPalette();
});
prompt?.addEventListener("focus", () => {
  void availableSkills().catch(() => {});
});
prompt?.addEventListener("paste", (event) => {
  const images = [...event.clipboardData.files].filter((file) =>
    file.type.startsWith("image/"),
  );
  if (images.length) addImages(images);
});

document.addEventListener("keydown", (event) => {
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
    event.preventDefault();
    prompt.focus();
    if (!prompt.value.startsWith("/")) prompt.value = "/";
    void renderCommandPalette();
  } else if (event.key === "Escape" && !document.querySelector("#settings-modal")?.classList.contains("hidden")) {
    closeModal("settings-modal");
  } else if (event.key === "Escape" && !document.querySelector("#goal-modal")?.classList.contains("hidden")) {
    closeModal("goal-modal");
  } else if (event.key === "Escape" && !document.querySelector("#command-palette")?.hidden) {
    document.querySelector("#command-palette").hidden = true;
  } else if (event.key === "Escape" && sidebar?.classList.contains("mobile-open")) {
    setMobileSidebar(false);
  }
});

function resizePrompt() {
  prompt.style.height = "auto";
  prompt.style.height = prompt.scrollHeight + "px";
}

document.querySelector("#attach-button")?.addEventListener("click", () => {
  document.querySelector("#image-input").click();
});
document.querySelector("#image-input")?.addEventListener("change", (event) => {
  addImages([...event.target.files]);
  event.target.value = "";
});

function addImages(files) {
  for (const file of files) {
    const reader = new FileReader();
    reader.onload = () => {
      attachments.push({ name: file.name, url: reader.result });
      renderAttachments();
    };
    reader.readAsDataURL(file);
  }
}

function renderAttachments() {
  attachmentsNode.replaceChildren();
  attachments.forEach((attachment, index) => {
    const wrapper = document.createElement("div");
    wrapper.className = "relative";
    const image = document.createElement("img");
    image.className = "size-16 rounded-lg border border-border object-cover";
    image.src = attachment.url;
    image.alt = attachment.name;
    const remove = document.createElement("button");
    remove.type = "button";
    remove.className =
      "absolute -right-1 -top-1 grid size-5 place-items-center rounded-full bg-primary text-xs text-primary-foreground";
    remove.textContent = "×";
    remove.ariaLabel = "Remove attachment";
    remove.onclick = () => {
      attachments.splice(index, 1);
      renderAttachments();
    };
    wrapper.append(image, remove);
    attachmentsNode.append(wrapper);
  });
}

stopButton?.addEventListener("click", async () => {
  if (threadId && turnId) {
    queuePaused = queuedDrafts.length > 0;
    void persistQueue();
    setRunState("stopping");
    try {
      await rpc("turn/interrupt", { threadId, turnId });
    } catch (error) {
      setRunState("running");
      showError(error.message);
    }
  }
});

document.querySelector("#queue-panel")?.addEventListener("click", (event) => {
  const remove = event.target.closest("[data-queue-index]");
  if (remove) {
    const [draft] = queuedDrafts.splice(Number(remove.dataset.queueIndex), 1);
    if (draft) void deletePersistedDraft(draft);
    void persistQueue();
    renderQueue();
    return;
  }
  const up = event.target.closest("[data-queue-up]");
  const down = event.target.closest("[data-queue-down]");
  const index = Number(up?.dataset.queueUp ?? down?.dataset.queueDown);
  if (!Number.isFinite(index)) return;
  const destination = up ? index - 1 : index + 1;
  [queuedDrafts[index], queuedDrafts[destination]] = [queuedDrafts[destination], queuedDrafts[index]];
  void persistQueue();
  renderQueue();
});
document.querySelector("#clear-queue-button")?.addEventListener("click", () => {
  queuedDrafts.forEach((draft) => void deletePersistedDraft(draft));
  queuedDrafts.splice(0);
  queuePaused = false;
  void persistQueue();
  renderQueue();
});
document.querySelector("#run-queue-button")?.addEventListener("click", () => {
  queuePaused = false;
  void persistQueue();
  sendNextQueued();
});

function effortLabel(effort) {
  if (effort === "xhigh") return "XHigh";
  return effort ? effort[0].toUpperCase() + effort.slice(1) : "Default";
}

function refreshEfforts(preferredEffort) {
  if (!modelSelect || !effortSelect) return;
  const option = modelSelect.selectedOptions[0];
  const efforts = (option?.dataset.efforts || "").split(",").filter(Boolean);
  const fallback = option?.dataset.defaultEffort || efforts[0] || "";
  const selected = efforts.includes(preferredEffort) ? preferredEffort : fallback;
  effortSelect.replaceChildren(
    ...efforts.map((effort) => {
      const effortOption = document.createElement("option");
      effortOption.value = effort;
      effortOption.textContent = effortLabel(effort);
      effortOption.selected = effort === selected;
      return effortOption;
    }),
  );
  syncMobileThreadControls();
}

function syncMobileThreadControls() {
  if (mobileModelSelect && modelSelect) mobileModelSelect.value = modelSelect.value;
  if (mobileEffortSelect && effortSelect) {
    mobileEffortSelect.replaceChildren(...[...effortSelect.options].map((option) => option.cloneNode(true)));
    mobileEffortSelect.value = effortSelect.value;
  }
  if (mobileModeSelect && modeSelect) mobileModeSelect.value = modeSelect.value;
}

function currentThreadSettings() {
  const model = modelSelect?.value;
  if (!model) return {};
  const effort = effortSelect?.value || null;
  const mode = modeSelect?.value || "default";
  return {
    model,
    effort,
    collaborationMode: {
      mode,
      settings: {
        model,
        reasoning_effort: effort,
        developer_instructions: null,
      },
    },
  };
}

async function updateThreadSettings() {
  if (!threadId) return;
  try {
    await rpc("thread/settings/update", {
      threadId,
      ...currentThreadSettings(),
    });
  } catch (error) {
    showError(error.message);
  }
}

modelSelect?.addEventListener("change", () => {
  refreshEfforts(modelSelect.selectedOptions[0]?.dataset.defaultEffort || "");
  void updateThreadSettings();
});
effortSelect?.addEventListener("change", () => {
  syncMobileThreadControls();
  void updateThreadSettings();
});
modeSelect?.addEventListener("change", () => {
  const modeEffort = modeSelect.selectedOptions[0]?.dataset.effort;
  const modelDefault = modelSelect?.selectedOptions[0]?.dataset.defaultEffort || "";
  refreshEfforts(modeEffort || modelDefault);
  localStorage.setItem(`codex-web-mode:${threadId || "new"}`, modeSelect.value);
  syncComposerPlanToggle();
  syncMobileThreadControls();
  void updateThreadSettings();
});
mobileModelSelect?.addEventListener("change", () => {
  modelSelect.value = mobileModelSelect.value;
  modelSelect.dispatchEvent(new Event("change"));
});
mobileEffortSelect?.addEventListener("change", () => {
  effortSelect.value = mobileEffortSelect.value;
  effortSelect.dispatchEvent(new Event("change"));
});
mobileModeSelect?.addEventListener("change", () => {
  modeSelect.value = mobileModeSelect.value;
  modeSelect.dispatchEvent(new Event("change"));
});

composerPlanToggle?.addEventListener("change", () => {
  if (!modeSelect) return;
  modeSelect.value = composerPlanToggle.checked ? "plan" : "default";
  modeSelect.dispatchEvent(new Event("change"));
});

function syncComposerPlanToggle() {
  if (!composerPlanToggle || !modeSelect) return;
  const supportsPlan = [...modeSelect.options].some((option) => option.value === "plan");
  document.querySelector("#composer-plan-control").hidden = !supportsPlan;
  composerPlanToggle.checked = supportsPlan && modeSelect.value === "plan";
}

function restoreModeSelection() {
  if (!modeSelect) return;
  const storedMode = localStorage.getItem(`codex-web-mode:${threadId || "new"}`);
  if (storedMode && [...modeSelect.options].some((option) => option.value === storedMode)) {
    modeSelect.value = storedMode;
  }
  syncComposerPlanToggle();
}
restoreModeSelection();
syncMobileThreadControls();

const threadSearch = document.querySelector("#thread-search");
const cwdOnlyToggle = document.querySelector("#cwd-only-toggle");
const cwdFilterStorageKey = "codex-web-cwd-only";

function normalizedPath(value) {
  return (value || "").replaceAll("\\", "/").replace(/\/+$/, "");
}

function filterThreads() {
  const query = threadSearch?.value.toLowerCase() || "";
  const workspaceCwd = normalizedPath(document.body.dataset.workspaceCwd);
  for (const link of document.querySelectorAll("[data-thread-title]")) {
    const titleMatches = link.dataset.threadTitle.includes(query);
    const cwdMatches =
      !cwdOnlyToggle?.checked ||
      normalizedPath(link.dataset.threadCwd) === workspaceCwd;
    link.hidden = !titleMatches || !cwdMatches;
  }
}

if (cwdOnlyToggle) {
  const storedCwdFilter = localStorage.getItem(cwdFilterStorageKey);
  cwdOnlyToggle.checked = storedCwdFilter === null || storedCwdFilter === "true";
  cwdOnlyToggle.addEventListener("change", () => {
    localStorage.setItem(cwdFilterStorageKey, String(cwdOnlyToggle.checked));
    filterThreads();
  });
}
threadSearch?.addEventListener("input", filterThreads);
filterThreads();

const appShell = document.querySelector("#app-shell");
const sidebar = document.querySelector("#sidebar");
const sidebarBackdrop = document.querySelector("#sidebar-backdrop");
const sidebarCollapseButton = document.querySelector("#sidebar-collapse-button");
const sidebarStorageKey = "codex-web-sidebar-collapsed";

function setSidebarCollapsed(collapsed) {
  appShell?.classList.toggle("sidebar-collapsed", collapsed);
  sidebarCollapseButton?.setAttribute("aria-expanded", String(!collapsed));
  if (sidebarCollapseButton) {
    sidebarCollapseButton.ariaLabel = collapsed ? "Expand navigation" : "Collapse navigation";
    sidebarCollapseButton.title = sidebarCollapseButton.ariaLabel;
  }
  localStorage.setItem(sidebarStorageKey, String(collapsed));
}

function setMobileSidebar(open) {
  sidebar?.classList.toggle("mobile-open", open);
  sidebarBackdrop?.classList.toggle("mobile-open", open);
  document.querySelector("#menu-button")?.setAttribute("aria-expanded", String(open));
  if (open) document.querySelector("#thread-search")?.focus();
}

setSidebarCollapsed(localStorage.getItem(sidebarStorageKey) === "true");
sidebarCollapseButton?.addEventListener("click", () => setSidebarCollapsed(!appShell.classList.contains("sidebar-collapsed")));
document.querySelector("#menu-button")?.addEventListener("click", () => setMobileSidebar(true));
document.querySelector("#sidebar-close-button")?.addEventListener("click", () => setMobileSidebar(false));
sidebarBackdrop?.addEventListener("click", () => setMobileSidebar(false));

async function loadSession(url, activeLink, { updateHistory = true } = {}) {
  sessionNavigationController?.abort();
  const controller = new AbortController();
  sessionNavigationController = controller;
  const loading = document.querySelector("#session-loading");
  loading.hidden = false;
  document.querySelector("#chat-main").ariaBusy = "true";
  activeLink.classList.add("bg-foreground/[0.07]");
  const requestedThreadId = activeLink.dataset.threadId || "new";
  navigationState = { threadId: requestedThreadId === "new" ? "" : requestedThreadId, events: [] };
  try {
    const response = await fetch(
      `${base}/api/session/${encodeURIComponent(requestedThreadId)}`,
      { signal: controller.signal },
    );
    if (!response.ok) throw new Error("Could not load this conversation");
    const session = await response.json();
    transcript.innerHTML = session.html;
    installHistoryLoader(session.nextCursor);
    threadId = session.threadId || "";
    turnId = session.activeTurnId || "";
    document.body.dataset.threadId = threadId;
    document.body.dataset.activeTurnId = turnId;
    composer.dataset.threadId = threadId;
    for (const card of transcript.querySelectorAll("[data-plan-proposal]")) {
      applyStoredPlanDecision(card);
    }
    await restoreQueue(threadId);
    document.querySelector("#thread-title").textContent = session.title;
    document.querySelector("#thread-cwd").textContent = session.cwd;
    if (session.cwd && document.body.dataset.workspaceCwd !== session.cwd) {
      document.body.dataset.workspaceCwd = session.cwd;
      skillCache = undefined;
      skillCacheCwd = undefined;
      skillLoadErrors = [];
    }
    if (session.model && modelSelect) {
      const modelOption = [...modelSelect.options].find(
        (option) => option.value === session.model,
      );
      if (modelOption) modelSelect.value = modelOption.value;
      refreshEfforts(session.reasoningEffort || "");
    }
    syncPermissionMode(session.permissionMode || "workspace");
    restoreModeSelection();
    filterThreads();
    for (const link of document.querySelectorAll("[data-session-link]")) {
      link.classList.toggle("bg-foreground/[0.07]", link === activeLink);
    }
    if (updateHistory) history.pushState({}, "", url);
    document.title = `${session.title} - Codex`;
    loading.hidden = true;
    document.querySelector("#chat-main").ariaBusy = "false";
    setRunState(turnId ? "running" : "idle");
    const buffered = navigationState?.events || [];
    navigationState = undefined;
    buffered
      .filter((event) => !event.seq || event.seq > Number(session.baseSeq || 0))
      .forEach(applyEvent);
    scrollToLatest();
  } catch (error) {
    if (error.name === "AbortError") return;
    loading.hidden = true;
    document.querySelector("#chat-main").ariaBusy = "false";
    activeLink.classList.remove("bg-foreground/[0.07]");
    navigationState = undefined;
    showError(error.message);
  }
}

async function reloadCurrentSession() {
  const link = threadLink(threadId) || document.querySelector("[data-thread-id='']");
  if (link) await loadSession(link.href, link, { updateHistory: false });
}

function installHistoryLoader(cursor) {
  document.querySelector("[data-history-loader]")?.remove();
  if (!cursor) return;
  const wrapper = document.createElement("div");
  wrapper.className = "mb-6 flex justify-center";
  wrapper.dataset.historyLoader = "";
  const button = document.createElement("button");
  button.type = "button";
  button.className = "rounded-full border border-border bg-background px-3 py-1.5 text-xs text-muted-foreground shadow-xs hover:text-foreground";
  button.dataset.loadEarlier = cursor;
  button.textContent = "Load earlier messages";
  wrapper.append(button);
  transcript.prepend(wrapper);
}

transcript?.addEventListener("click", async (event) => {
  const button = event.target.closest("[data-load-earlier]");
  if (!button || !threadId) return;
  button.disabled = true;
  button.textContent = "Loading earlier messages…";
  const previousHeight = transcript.scrollHeight;
  try {
    const response = await fetch(
      `${base}/api/session/${encodeURIComponent(threadId)}/turns?cursor=${encodeURIComponent(button.dataset.loadEarlier)}`,
    );
    if (!response.ok) throw new Error("Could not load earlier messages");
    const page = await response.json();
    const template = document.createElement("template");
    template.innerHTML = page.html;
    button.closest("[data-history-loader]").after(template.content);
    installHistoryLoader(page.nextCursor);
    transcript.scrollTop += transcript.scrollHeight - previousHeight;
  } catch (error) {
    button.disabled = false;
    button.textContent = "Try loading earlier messages again";
    showError(error.message);
  }
});

window.addEventListener("popstate", () => {
  const activeLink = [...document.querySelectorAll("[data-session-link]")].find(
    (link) => link.href === location.href,
  );
  if (activeLink) void loadSession(location.href, activeLink, { updateHistory: false });
});

document.querySelector("#sidebar")?.addEventListener("click", (event) => {
  const sessionLink = event.target.closest("[data-session-link]");
  if (
    sessionLink &&
    event.button === 0 &&
    !event.metaKey &&
    !event.ctrlKey &&
    !event.shiftKey &&
    !event.altKey
  ) {
    event.preventDefault();
    void loadSession(sessionLink.href, sessionLink);
    setMobileSidebar(false);
    return;
  }
});

async function authenticateMcp(button) {
  const loginWindow = window.open("", "_blank");
  if (loginWindow) {
    loginWindow.document.title = "Connecting MCP server";
    loginWindow.document.body.textContent = "Preparing secure login…";
  }
  button.disabled = true;
  button.textContent = "Opening…";
  try {
    const result = await rpc("mcpServer/oauth/login", {
      name: button.dataset.mcpLogin,
      threadId: threadId || null,
    });
    const authorizationUrl = result.authorizationUrl || result.authorization_url;
    if (!authorizationUrl) throw new Error("MCP login did not return an authorization URL");
    if (loginWindow) {
      loginWindow.location.replace(authorizationUrl);
    } else {
      window.location.assign(authorizationUrl);
    }
    button.textContent = "Waiting…";
  } catch (error) {
    loginWindow?.close();
    button.disabled = false;
    button.textContent = "Retry";
    showError(error.message);
  }
}

document.querySelector("#settings-modal")?.addEventListener("click", (event) => {
  const button = event.target.closest("[data-mcp-login]");
  if (button) void authenticateMcp(button);
});

const PLAN_IMPLEMENTATION_MESSAGE = "Implement the plan.";
const PLAN_CLEAR_CONTEXT_PREFIX =
  "A previous agent produced the plan below to accomplish the user's task. " +
  "Implement the plan in a fresh context. Treat the plan as the source of " +
  "user intent, re-read files as needed, and carry the work through " +
  "implementation and verification.";

function planDecisionKey(card) {
  return `codex-web-plan-decision:${instanceId}:${threadId}:${card.dataset.itemId}`;
}

function storedPlanDecision(card) {
  return sessionStorage.getItem(planDecisionKey(card));
}

function showPlanDecision(card, decision) {
  const actions = card.querySelector("[data-plan-actions]");
  const buttons = card.querySelector("[data-plan-buttons]");
  const status = card.querySelector("[data-plan-status]");
  if (!actions) return;
  if (decision === "stay") {
    actions.hidden = true;
    card.dataset.planActionable = "false";
    return;
  }
  if (decision === "submitted") {
    actions.hidden = false;
    buttons.hidden = true;
    status.textContent = "Implementation started.";
    card.dataset.planActionable = "false";
  }
}

function applyStoredPlanDecision(card) {
  const decision = storedPlanDecision(card);
  if (decision) showPlanDecision(card, decision);
}

function deactivatePlanProposals(except) {
  for (const card of transcript.querySelectorAll("[data-plan-proposal]")) {
    if (card === except) continue;
    card.dataset.planActionable = "false";
    const actions = card.querySelector("[data-plan-actions]");
    if (actions) actions.hidden = true;
  }
}

function activateLatestPlanProposal(completedTurnId) {
  const candidates = [...transcript.querySelectorAll("[data-plan-proposal][data-plan-complete='true']")];
  const card = candidates
    .reverse()
    .find((candidate) => candidate.dataset.planTurnId === completedTurnId);
  if (!card) return;
  deactivatePlanProposals(card);
  if (storedPlanDecision(card)) {
    applyStoredPlanDecision(card);
    return;
  }
  card.dataset.planActionable = "true";
  card.querySelector("[data-plan-actions]").hidden = false;
}

function setPlanActionPending(card, message) {
  card.querySelector("[data-plan-status]").textContent = message;
  for (const button of card.querySelectorAll("[data-plan-action]")) {
    button.disabled = true;
  }
}

function resetPlanAction(card, message) {
  card.querySelector("[data-plan-status]").textContent = message;
  for (const button of card.querySelectorAll("[data-plan-action]")) {
    button.disabled = false;
  }
}

function selectCollaborationMode(mode, { updateThread = true } = {}) {
  const option = [...(modeSelect?.options || [])].find(
    (candidate) => candidate.value === mode,
  );
  if (!option) throw new Error(`${mode === "default" ? "Default" : "Plan"} mode is unavailable`);
  modeSelect.value = mode;
  if (updateThread) {
    modeSelect.dispatchEvent(new Event("change"));
  } else {
    const modelDefault = modelSelect?.selectedOptions[0]?.dataset.defaultEffort || "";
    refreshEfforts(option.dataset.effort || modelDefault);
    syncComposerPlanToggle();
  }
}

async function implementPlanInCurrentThread() {
  selectCollaborationMode("default");
  const draft = makeDraft(
    PLAN_IMPLEMENTATION_MESSAGE,
    [],
    globalThis.crypto?.randomUUID?.() || `web-${Date.now()}`,
  );
  return await sendDraft(draft);
}

async function implementPlanInFreshThread(card) {
  selectCollaborationMode("default", { updateThread: false });
  let freshThreadId = card.dataset.freshThreadId;
  if (!freshThreadId) {
    const result = await rpc("thread/start", {
      ephemeral: false,
      cwd: document.querySelector("#thread-cwd")?.textContent || document.body.dataset.workspaceCwd,
      sessionStartSource: "clear",
      ...permissionSettings(permissionsSelect?.value || "workspace"),
    });
    freshThreadId = result.thread.id;
    card.dataset.freshThreadId = freshThreadId;
    localStorage.setItem(`codex-web-mode:${freshThreadId}`, "default");
  }
  const text = `${PLAN_CLEAR_CONTEXT_PREFIX}\n\n${card.dataset.planMarkdown}`;
  const clientUserMessageId =
    globalThis.crypto?.randomUUID?.() || `web-${Date.now()}`;
  await rpc("turn/start", {
    threadId: freshThreadId,
    clientUserMessageId,
    input: [{ type: "text", text, text_elements: [] }],
    ...currentThreadSettings(),
  });
  location.assign(`${base}/thread/${encodeURIComponent(freshThreadId)}`);
  return true;
}

for (const card of transcript.querySelectorAll("[data-plan-proposal]")) {
  applyStoredPlanDecision(card);
}

transcript?.addEventListener("click", async (event) => {
  const button = event.target.closest("[data-plan-action]");
  if (!button) return;
  const card = button.closest("[data-plan-proposal]");
  if (!card || card.dataset.planActionable !== "true") return;
  const action = button.dataset.planAction;
  if (action === "stay") {
    try {
      selectCollaborationMode("plan");
      sessionStorage.setItem(planDecisionKey(card), "stay");
      showPlanDecision(card, "stay");
      prompt.focus();
    } catch (error) {
      resetPlanAction(card, error.message);
    }
    return;
  }

  setPlanActionPending(
    card,
    action === "fresh" ? "Starting a fresh implementation thread…" : "Starting implementation…",
  );
  try {
    const submitted = action === "fresh"
      ? await implementPlanInFreshThread(card)
      : await implementPlanInCurrentThread();
    if (!submitted) {
      selectCollaborationMode("plan");
      resetPlanAction(card, "Could not start implementation. Try again.");
      return;
    }
    sessionStorage.setItem(planDecisionKey(card), "submitted");
    showPlanDecision(card, "submitted");
  } catch (error) {
    if (action === "fresh") {
      selectCollaborationMode("plan", { updateThread: false });
    }
    resetPlanAction(card, error.message);
    showError(error.message);
  }
});

transcript?.addEventListener("click", async (event) => {
  const decisionButton = event.target.closest("[data-decision]");
  if (!decisionButton) return;
  const card = decisionButton.closest("[data-request-id]");
  const id = parseRequestId(card.dataset.requestId);
  const method = card.dataset.method;
  const decision = decisionButton.dataset.decision;
  const params = JSON.parse(card.dataset.params);
  decisionButton.disabled = true;
  try {
    let result;
    if (method === "item/tool/requestUserInput") {
      const answers = {};
      for (const question of card.querySelectorAll("[data-question-id]")) {
        const answer = question.querySelector("[data-user-answer]").value.trim();
        if (!answer) throw new Error("Answer every question before continuing");
        answers[question.dataset.questionId] = { answers: [answer] };
      }
      result = { answers };
    } else if (method === "item/permissions/requestApproval") {
      result = {
        permissions: decision === "decline" ? {} : params.permissions,
        scope: decision === "acceptForSession" ? "session" : "turn",
      };
    } else {
      result = { decision };
    }
    await answerRequest(id, result);
    card.remove();
  } catch (error) {
    decisionButton.disabled = false;
    showError(error.message);
  }
});

function parseRequestId(id) {
  if (/^-?\d+$/.test(id)) return Number(id);
  return id;
}

function showError(message) {
  const node = document.createElement("div");
  node.className =
    "mb-4 rounded-lg border border-destructive/30 bg-destructive/[0.04] p-3 text-sm text-destructive";
  node.textContent = message;
  transcript.append(node);
}

connectEvents();
setRunState(runState);
void restoreQueue(threadId).then(() => {
  for (const node of document.querySelectorAll("[data-client-id]")) {
    if (node.dataset.clientId) acknowledgeDraft(node.dataset.clientId);
  }
});
scrollToLatest();

const composerDock = document.querySelector("#composer-dock");
if (composerDock && globalThis.ResizeObserver) {
  new ResizeObserver(([entry]) => {
    transcript.style.paddingBottom = `${entry.contentRect.height + 24}px`;
  }).observe(composerDock);
}
