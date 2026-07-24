const settingsBase = document.body.dataset.basePath;
let mcpConfig = {};
let mcpEffective = {};
let mcpOrigins = {};
let mcpExpectedVersion = null;
const modalReturnFocus = new WeakMap();

async function settingsRpc(method, params) {
  const response = await fetch(settingsBase + "/api/rpc", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ method, params: params || {} }),
  });
  const envelope = await response.json();
  if (!response.ok || envelope.error) throw new Error(envelope.error?.message || envelope.error || "Request failed");
  return envelope.result;
}

async function settingsApi(path, payload) {
  const response = await fetch(settingsBase + path, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });
  const result = await response.json();
  if (!response.ok || result.error) throw new Error(result.error || "Request failed");
  return result;
}

function setCodexTheme(theme) {
  const value = ["light", "dark", "system"].includes(theme) ? theme : "dark";
  localStorage.setItem("codex-web-theme", value);
  document.documentElement.classList.toggle("light", value === "light");
  document.documentElement.classList.toggle("dark", value === "dark" || (value === "system" && matchMedia("(prefers-color-scheme: dark)").matches));
}

function openCodexModal(id) {
  const modal = document.getElementById(id);
  if (!modal) return;
  modalReturnFocus.set(modal, document.activeElement);
  modal.classList.remove("hidden");
  modal.classList.add("grid");
  requestAnimationFrame(() => {
    const target = modal.querySelector(
      "[autofocus], button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), a[href]",
    );
    target?.focus();
  });
}

function closeCodexModal(id) {
  const modal = document.getElementById(id);
  if (!modal) return;
  modal.classList.add("hidden");
  modal.classList.remove("grid");
  modalReturnFocus.get(modal)?.focus();
  modalReturnFocus.delete(modal);
}

function message(selector, text, error = false) {
  const node = document.querySelector(selector);
  if (!node) return;
  node.textContent = text;
  node.classList.remove("hidden", "text-destructive", "text-muted-foreground");
  node.classList.add(error ? "text-destructive" : "text-muted-foreground");
}

const accountMessage = (text, error = false) => message("[data-account-message]", text, error);
const mcpMessage = (text, error = false) => message("[data-mcp-message]", text, error);
const sshMessage = (text, error = false) => message("[data-ssh-message]", text, error);
const accessMessage = (text, error = false) => message("[data-access-message]", text, error);

async function updateAccountProfiles(payload) {
  await settingsApi("/api/account-profiles", payload);
  location.reload();
}

function accountConnectionPayload(form) {
  const connection = form.elements.connection.value;
  return {
    proxy: connection === "proxy" ? form.elements.proxy.value || null : null,
    useSshTunnel: connection === "tunnel",
  };
}

function syncAccountConnection(form) {
  const proxy = form.querySelector("[data-account-proxy-field]");
  const visible = form.elements.connection.value === "proxy";
  proxy?.classList.toggle("hidden", !visible);
  proxy?.classList.toggle("grid", visible);
}

function sshConfig(form) {
  return {
    destination: form.elements.destination.value.trim(),
    identityFile: form.elements.identityFile.value.trim() || null,
    sshPort: form.elements.sshPort.value ? Number(form.elements.sshPort.value) : null,
    localPort: form.elements.localPort.value ? Number(form.elements.localPort.value) : null,
  };
}

function renderSshStatus(result) {
  const state = document.querySelector("[data-ssh-state]");
  if (state) {
    state.textContent = result.state === "connected" ? `Connected${result.proxyUrl ? ` · ${result.proxyUrl}` : ""}` : result.state === "failed" ? "Needs attention" : "Not configured";
    state.classList.toggle("text-emerald-400", result.state === "connected");
    state.classList.toggle("text-destructive", result.state === "failed");
  }
  const form = document.querySelector("[data-ssh-tunnel-form]");
  if (!form || !result.config) return;
  form.elements.destination.value = result.config.destination || "";
  form.elements.identityFile.value = result.config.identityFile || "";
  form.elements.sshPort.value = result.config.sshPort || "";
  form.elements.localPort.value = result.config.localPort || "";
}

async function loadSshStatus() {
  const result = await settingsApi("/api/ssh-tunnel", { action: "status" });
  renderSshStatus(result);
}

async function copyText(value) {
  if (navigator.clipboard?.writeText) return navigator.clipboard.writeText(value);
  const input = document.createElement("textarea");
  input.value = value;
  input.style.position = "fixed";
  input.style.opacity = "0";
  document.body.append(input);
  input.select();
  const copied = document.execCommand("copy");
  input.remove();
  if (!copied) throw new Error("Your browser blocked clipboard access");
}

async function accountAuth(action, name, extra = {}) {
  return settingsApi("/api/account-auth", { action, name, ...extra });
}

async function refreshAccountStatus(name) {
  const node = document.querySelector(`[data-account-status="${CSS.escape(name)}"]`);
  if (!node) return;
  try {
    const result = await accountAuth("status", name);
    const account = result.account;
    if (!account) {
      node.textContent = "Not signed in";
      return;
    }
    const used = result.rateLimits?.primary?.usedPercent;
    const plan = account.planType || account.type || "Signed in";
    node.textContent = used == null ? plan : `${plan} · ${used}% used`;
  } catch (error) {
    node.textContent = error.message;
  }
}

async function startChatgptLogin(name) {
  const result = await accountAuth("chatgptDeviceCode", name);
  accountMessage(`Enter ${result.userCode} in the ChatGPT window. Waiting for ${name} to connect…`);
  window.open(result.verificationUrl, "_blank", "noopener,noreferrer");
  const poll = setInterval(async () => {
    try {
      const status = await accountAuth("status", name);
      if (status.account) {
        clearInterval(poll);
        await refreshAccountStatus(name);
        accountMessage(`${name} is connected.`);
      }
    } catch {}
  }, 1500);
  setTimeout(() => clearInterval(poll), 180000);
}

async function loadMcpConfig() {
  const result = await settingsRpc("config/read", { includeLayers: true });
  mcpOrigins = result.origins || {};
  const user = (result.layers || []).find((layer) => layer.name?.type === "user" && !layer.name?.profile);
  mcpEffective = structuredClone(result.config?.mcp_servers || {});
  mcpConfig = structuredClone(user?.config?.mcp_servers || {});
  mcpExpectedVersion = user?.version || null;
}

function lines(value) {
  return Array.isArray(value) ? value.join("\n") : "";
}

function parseLines(value) {
  const items = value.split("\n").map((item) => item.trim()).filter(Boolean);
  return items.length ? items : undefined;
}

function parseObject(value, label) {
  if (!value.trim()) return undefined;
  const parsed = JSON.parse(value);
  if (!parsed || Array.isArray(parsed) || typeof parsed !== "object") throw new Error(`${label} must be a JSON object`);
  return parsed;
}

function parseArray(value, label) {
  if (!value.trim()) return undefined;
  const parsed = JSON.parse(value);
  if (!Array.isArray(parsed)) throw new Error(`${label} must be a JSON array`);
  return parsed;
}

function setMcpTransport(form) {
  const http = form.elements.transport.value === "http";
  form.querySelectorAll("[data-mcp-http]").forEach((node) => node.classList.toggle("hidden", !http));
  form.querySelectorAll("[data-mcp-http]").forEach((node) => node.classList.toggle("grid", http));
  form.querySelectorAll("[data-mcp-stdio]").forEach((node) => node.classList.toggle("hidden", http));
}

function mcpOrigin(name) {
  return mcpOrigins[`mcp_servers.${name}`] || mcpOrigins.mcp_servers;
}

function openMcpEditor(name = "") {
  const form = document.querySelector("[data-mcp-form]");
  const userOwned = !name || Object.hasOwn(mcpConfig, name);
  const config = name ? (mcpConfig[name] || mcpEffective[name] || {}) : {};
  const origin = name ? mcpOrigin(name) : null;
  if (!userOwned || (origin && origin.name?.type && origin.name.type !== "user")) {
    mcpMessage(`${name} is managed by ${origin?.name?.type || "a plugin"} and is read-only.`, true);
    return;
  }
  form.reset();
  form.elements.originalName.value = name;
  form.elements.name.value = name;
  form.elements.enabled.checked = config.enabled !== false;
  const isHttp = Boolean(config.url);
  form.elements.transport.value = isHttp ? "http" : "stdio";
  form.elements.command.value = config.command || "";
  form.elements.args.value = lines(config.args);
  form.elements.cwd.value = config.cwd || "";
  form.elements.url.value = config.url || "";
  form.elements.bearerTokenEnvVar.value = config.bearer_token_env_var || "";
  form.elements.env.value = JSON.stringify(isHttp ? (config.http_headers || {}) : (config.env || {}), null, 2);
  form.elements.envHttpHeaders.value = JSON.stringify(config.env_http_headers || {}, null, 2);
  form.elements.startupTimeoutSec.value = config.startup_timeout_sec ?? "";
  form.elements.toolTimeoutSec.value = config.tool_timeout_sec ?? "";
  form.elements.enabledTools.value = lines(config.enabled_tools);
  form.elements.disabledTools.value = lines(config.disabled_tools);
  form.elements.environmentId.value = config.environment_id || "";
  form.elements.auth.value = config.auth || "oauth";
  form.elements.defaultToolsApprovalMode.value = config.default_tools_approval_mode || "";
  form.elements.required.checked = config.required === true;
  form.elements.supportsParallelToolCalls.checked = config.supports_parallel_tool_calls === true;
  form.elements.scopes.value = lines(config.scopes);
  form.elements.oauthResource.value = config.oauth_resource || "";
  form.elements.envVars.value = JSON.stringify(config.env_vars || [], null, 2);
  form.elements.oauth.value = JSON.stringify(config.oauth || {}, null, 2);
  form.elements.tools.value = JSON.stringify(config.tools || {}, null, 2);
  setMcpTransport(form);
  form.classList.remove("hidden");
  form.classList.add("grid");
  form.scrollIntoView({ behavior: "smooth", block: "nearest" });
}

async function saveMcpConfig(next) {
  const result = await settingsRpc("config/value/write", {
    keyPath: "mcp_servers",
    value: next,
    mergeStrategy: "replace",
    expectedVersion: mcpExpectedVersion,
  });
  mcpExpectedVersion = result.version || mcpExpectedVersion;
  await settingsRpc("config/mcpServer/reload", {});
  mcpConfig = next;
}

setCodexTheme(localStorage.getItem("codex-web-theme") || "dark");
globalThis.codexWebSettings = { setTheme: setCodexTheme, openModal: openCodexModal, closeModal: closeCodexModal };

const settingsModal = document.querySelector("#settings-modal");
document.querySelector("#settings-button")?.addEventListener("click", async () => {
  openCodexModal("settings-modal");
  document.querySelectorAll("[data-account-status]").forEach((node) => void refreshAccountStatus(node.dataset.accountStatus));
  try { await loadMcpConfig(); } catch (error) { mcpMessage(error.message, true); }
  try { await loadSshStatus(); } catch (error) { sshMessage(error.message, true); }
});
document.querySelectorAll("[data-close-modal]").forEach((button) => button.addEventListener("click", () => closeCodexModal(button.dataset.closeModal)));
settingsModal?.addEventListener("click", (event) => { if (event.target === settingsModal) closeCodexModal("settings-modal"); });
const settingsTheme = document.querySelector("#settings-theme");
if (settingsTheme) {
  settingsTheme.value = localStorage.getItem("codex-web-theme") || "dark";
  settingsTheme.addEventListener("change", () => setCodexTheme(settingsTheme.value));
}

document.querySelector("[data-account-profile-form]")?.addEventListener("submit", (event) => {
  event.preventDefault();
  const data = new FormData(event.currentTarget);
  void updateAccountProfiles({ action: "add", name: data.get("name"), label: data.get("label"), ...accountConnectionPayload(event.currentTarget) }).catch((error) => accountMessage(error.message, true));
});

document.querySelector("[data-account-edit-form]")?.addEventListener("submit", (event) => {
  event.preventDefault();
  const data = new FormData(event.currentTarget);
  void updateAccountProfiles({ action: "update", name: data.get("name"), label: data.get("label"), ...accountConnectionPayload(event.currentTarget) }).catch((error) => accountMessage(error.message, true));
});

document.querySelectorAll("[data-account-profile-form], [data-account-edit-form]").forEach((form) => {
  form.elements.connection?.addEventListener("change", () => syncAccountConnection(form));
  syncAccountConnection(form);
});

settingsModal?.addEventListener("click", async (event) => {
  const use = event.target.closest("[data-account-use]");
  const login = event.target.closest("[data-account-login]");
  const key = event.target.closest("[data-account-api-key]");
  const edit = event.target.closest("[data-account-edit]");
  const move = event.target.closest("[data-account-move]");
  const remove = event.target.closest("[data-account-remove]");
  try {
    if (use) {
      accountMessage(`Switching to ${use.dataset.accountUse}…`);
      await settingsApi("/api/account-activate", { name: use.dataset.accountUse });
      location.reload();
    } else if (login) {
      await startChatgptLogin(login.dataset.accountLogin);
    } else if (key) {
      const apiKey = prompt(`OpenAI API key for ${key.dataset.accountApiKey}`);
      if (apiKey) {
        await accountAuth("apiKey", key.dataset.accountApiKey, { apiKey });
        await refreshAccountStatus(key.dataset.accountApiKey);
      }
    } else if (edit) {
      const form = document.querySelector("[data-account-edit-form]");
      form.elements.name.value = edit.dataset.accountEdit;
      form.elements.label.value = edit.dataset.label;
      form.elements.proxy.value = edit.dataset.proxy;
      form.elements.connection.value = edit.dataset.connection;
      syncAccountConnection(form);
      form.classList.remove("hidden");
      form.classList.add("grid");
      form.scrollIntoView({ behavior: "smooth", block: "nearest" });
    } else if (move) {
      await updateAccountProfiles({ action: "move", name: move.dataset.accountMove, direction: move.dataset.direction });
    } else if (remove && confirm(`Remove the ${remove.dataset.accountRemove} fallback?`)) {
      await updateAccountProfiles({ action: "remove", name: remove.dataset.accountRemove });
    }
  } catch (error) {
    accountMessage(error.message, true);
  }
});

document.querySelector("[data-account-edit-close]")?.addEventListener("click", () => document.querySelector("[data-account-edit-form]")?.classList.add("hidden"));
document.querySelector("[data-account-logout-edit]")?.addEventListener("click", async () => {
  const name = document.querySelector("[data-account-edit-form]").elements.name.value;
  if (!confirm(`Sign out ${name}?`)) return;
  try { await accountAuth("logout", name); accountMessage(`${name} signed out.`); await refreshAccountStatus(name); } catch (error) { accountMessage(error.message, true); }
});

document.querySelector("[data-copy-private-link]")?.addEventListener("click", async () => {
  if (!confirm("Anyone with this link can use this server user's Codex credentials and access its files. Copy it?")) return;
  try {
    const result = await settingsApi("/api/private-link", {});
    await copyText(location.origin + result.path);
    accessMessage("Private link copied. Share it only with someone you trust.");
  } catch (error) {
    accessMessage(error.message, true);
  }
});

document.querySelector("[data-ssh-test]")?.addEventListener("click", async (event) => {
  const form = event.currentTarget.form;
  event.currentTarget.disabled = true;
  sshMessage("Opening a temporary SSH tunnel…");
  try {
    const result = await settingsApi("/api/ssh-tunnel", { action: "test", config: sshConfig(form) });
    sshMessage(`Connection succeeded through ${result.proxyUrl}.`);
  } catch (error) {
    sshMessage(error.message, true);
  } finally {
    event.currentTarget.disabled = false;
  }
});

document.querySelector("[data-ssh-tunnel-form]")?.addEventListener("submit", async (event) => {
  event.preventDefault();
  sshMessage("Applying tunnel and reconnecting affected accounts…");
  try {
    const result = await settingsApi("/api/ssh-tunnel", { action: "apply", config: sshConfig(event.currentTarget) });
    renderSshStatus(result);
    sshMessage("Managed SSH tunnel is connected and saved.");
  } catch (error) {
    sshMessage(error.message, true);
  }
});

document.querySelector("[data-ssh-disable]")?.addEventListener("click", async () => {
  if (!confirm("Disable the managed SSH tunnel? Accounts assigned to it must be changed first.")) return;
  try {
    const result = await settingsApi("/api/ssh-tunnel", { action: "apply", config: null });
    renderSshStatus(result);
    sshMessage("Managed SSH tunnel disabled.");
  } catch (error) {
    sshMessage(error.message, true);
  }
});

document.querySelector("[data-mcp-add]")?.addEventListener("click", () => openMcpEditor());
document.querySelector("[data-mcp-close]")?.addEventListener("click", () => document.querySelector("[data-mcp-form]")?.classList.add("hidden"));
document.querySelector("[data-mcp-form] [name=transport]")?.addEventListener("change", (event) => setMcpTransport(event.target.form));

settingsModal?.addEventListener("click", async (event) => {
  const edit = event.target.closest("[data-mcp-edit]");
  const login = event.target.closest("[data-mcp-login]");
  if (edit) openMcpEditor(edit.dataset.mcpEdit);
  if (!login) return;
  const loginWindow = window.open("", "_blank");
  try {
    const result = await settingsRpc("mcpServer/oauth/login", { name: login.dataset.mcpLogin, threadId: document.body.dataset.threadId || null });
    if (!result.authorizationUrl) throw new Error("MCP login did not return an authorization URL");
    if (loginWindow) loginWindow.location.replace(result.authorizationUrl); else location.assign(result.authorizationUrl);
    login.textContent = "Waiting…";
  } catch (error) {
    loginWindow?.close();
    mcpMessage(error.message, true);
  }
});

document.querySelector("[data-mcp-form]")?.addEventListener("submit", async (event) => {
  event.preventDefault();
  const form = event.currentTarget;
  try {
    const name = form.elements.name.value.trim();
    const original = form.elements.originalName.value;
    const http = form.elements.transport.value === "http";
    const config = {
      enabled: form.elements.enabled.checked,
      startup_timeout_sec: form.elements.startupTimeoutSec.value ? Number(form.elements.startupTimeoutSec.value) : undefined,
      tool_timeout_sec: form.elements.toolTimeoutSec.value ? Number(form.elements.toolTimeoutSec.value) : undefined,
      enabled_tools: parseLines(form.elements.enabledTools.value),
      disabled_tools: parseLines(form.elements.disabledTools.value),
      environment_id: form.elements.environmentId.value.trim() || undefined,
      auth: form.elements.auth.value,
      required: form.elements.required.checked,
      supports_parallel_tool_calls: form.elements.supportsParallelToolCalls.checked,
      default_tools_approval_mode: form.elements.defaultToolsApprovalMode.value || undefined,
      scopes: parseLines(form.elements.scopes.value),
      oauth_resource: form.elements.oauthResource.value.trim() || undefined,
      env_vars: parseArray(form.elements.envVars.value, "Environment variables"),
      oauth: parseObject(form.elements.oauth.value, "OAuth client settings"),
      tools: parseObject(form.elements.tools.value, "Per-tool settings"),
    };
    if (http) {
      config.url = form.elements.url.value.trim();
      config.bearer_token_env_var = form.elements.bearerTokenEnvVar.value.trim() || undefined;
      config.http_headers = parseObject(form.elements.env.value, "Headers");
      config.env_http_headers = parseObject(form.elements.envHttpHeaders.value, "Environment-backed headers");
    } else {
      config.command = form.elements.command.value.trim();
      config.args = parseLines(form.elements.args.value);
      config.env = parseObject(form.elements.env.value, "Environment");
      config.cwd = form.elements.cwd.value.trim() || undefined;
    }
    Object.keys(config).forEach((key) => config[key] === undefined && delete config[key]);
    const next = structuredClone(mcpConfig);
    if (original && original !== name) delete next[original];
    next[name] = config;
    await saveMcpConfig(next);
    mcpMessage(`${name} saved and reloaded.`);
    form.classList.add("hidden");
    location.reload();
  } catch (error) {
    mcpMessage(error.message, true);
  }
});

document.querySelector("[data-mcp-remove]")?.addEventListener("click", async () => {
  const form = document.querySelector("[data-mcp-form]");
  const name = form.elements.originalName.value || form.elements.name.value;
  if (!name || !confirm(`Remove MCP server ${name}?`)) return;
  try { const next = structuredClone(mcpConfig); delete next[name]; await saveMcpConfig(next); location.reload(); } catch (error) { mcpMessage(error.message, true); }
});

document.querySelector("[data-mcp-logout]")?.addEventListener("click", async () => {
  const form = document.querySelector("[data-mcp-form]");
  const name = form.elements.originalName.value || form.elements.name.value;
  try { const result = await settingsRpc("mcpServer/oauth/logout", { name, threadId: document.body.dataset.threadId || null }); mcpMessage(result.removed ? `${name} OAuth credentials cleared.` : `${name} had no stored OAuth credentials.`); } catch (error) { mcpMessage(error.message, true); }
});

document.addEventListener("keydown", (event) => {
  const modal = [...document.querySelectorAll("#settings-modal, #goal-modal")]
    .find((candidate) => !candidate.classList.contains("hidden"));
  if (!modal) return;
  if (event.key === "Escape") {
    event.preventDefault();
    closeCodexModal(modal.id);
    return;
  }
  if (event.key !== "Tab") return;
  const focusable = [...modal.querySelectorAll(
    "button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), a[href]",
  )].filter((element) => !element.hidden && element.offsetParent !== null);
  if (!focusable.length) return;
  const first = focusable[0];
  const last = focusable.at(-1);
  if (event.shiftKey && document.activeElement === first) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first.focus();
  }
});
