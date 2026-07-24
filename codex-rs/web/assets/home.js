const picker = document.querySelector("[data-picker]");
const base = document.body.dataset.basePath;
const directoryList = document.querySelector("[data-directory-list]");
const pickerPath = document.querySelector("[data-picker-path]");
const pickerError = document.querySelector("[data-picker-error]");
const parentButton = document.querySelector("[data-parent-directory]");
let currentDirectory = "";
let parentDirectory = null;

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

function relativeTime(timestamp) {
  const seconds = Math.max(0, Math.floor(Date.now() / 1000) - Number(timestamp || 0));
  if (seconds < 60) return "now";
  if (seconds < 3600) return Math.floor(seconds / 60) + "m ago";
  if (seconds < 86400) return Math.floor(seconds / 3600) + "h ago";
  if (seconds < 604800) return Math.floor(seconds / 86400) + "d ago";
  return new Date(Number(timestamp) * 1000).toLocaleDateString();
}

function refreshRelativeTimes() {
  document.querySelectorAll("[data-relative-time]").forEach((node) => {
    node.textContent = relativeTime(node.dataset.relativeTime);
  });
}
refreshRelativeTimes();
setInterval(refreshRelativeTimes, 60000);

function setPickerError(message = "") {
  pickerError.textContent = message;
  pickerError.classList.toggle("hidden", !message);
}

function renderBreadcrumbs(path, roots) {
  const nav = document.querySelector("[data-breadcrumbs]");
  nav.replaceChildren();
  const root = roots.find((candidate) => path.startsWith(candidate)) || roots[0];
  const rootButton = document.createElement("button");
  rootButton.type = "button";
  rootButton.className = "shrink-0 rounded-md px-2 py-1 hover:bg-foreground/5";
  rootButton.textContent = root;
  rootButton.addEventListener("click", () => loadDirectory(root));
  nav.append(rootButton);
  const separator = path.includes("\\") ? "\\" : "/";
  const suffix = path.slice(root.length);
  const parts = suffix.split(/[\\/]/).filter(Boolean);
  let cursor = root;
  for (const part of parts) {
    const marker = document.createElement("span");
    marker.className = "text-muted-foreground";
    marker.textContent = "/";
    const button = document.createElement("button");
    button.type = "button";
    button.className = "shrink-0 rounded-md px-2 py-1 hover:bg-foreground/5";
    cursor = cursor.endsWith(separator) ? cursor + part : cursor + separator + part;
    const target = cursor;
    button.textContent = part;
    button.addEventListener("click", () => loadDirectory(target));
    nav.append(marker, button);
  }
}

async function loadDirectory(path = "", cursor = null, append = false) {
  setPickerError();
  if (!append) {
    directoryList.innerHTML = '<p class="px-3 py-8 text-center text-sm text-muted-foreground">Reading directories…</p>';
  }
  try {
    const response = await fetch(base + "/api/fs/list", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ path: path || null, cursor }),
    });
    const listing = await response.json();
    if (!response.ok || listing.error) throw new Error(listing.error || "Could not read this directory");
    currentDirectory = listing.path;
    parentDirectory = listing.parent;
    pickerPath.textContent = listing.path;
    pickerPath.title = listing.path;
    parentButton.disabled = !listing.parent;
    renderBreadcrumbs(listing.path, listing.roots);
    if (!append) directoryList.replaceChildren();
    if (!listing.entries.length) {
      directoryList.innerHTML = '<p class="px-3 py-8 text-center text-sm text-muted-foreground">No subdirectories here. You can still open this folder.</p>';
    }
    for (const entry of listing.entries) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-left hover:bg-foreground/5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring";
      const icon = document.createElement("span");
      icon.className = "font-mono text-xs text-sky-600 dark:text-sky-400";
      icon.textContent = entry.symlink ? "↗" : "⌁";
      const name = document.createElement("span");
      name.className = "min-w-0 flex-1 truncate text-sm";
      name.textContent = entry.name;
      button.append(icon, name);
      button.addEventListener("click", () => loadDirectory(entry.path));
      directoryList.append(button);
    }
    if (listing.nextCursor !== null) {
      const more = document.createElement("button");
      more.type = "button";
      more.className = "mt-1 w-full rounded-lg border border-border px-3 py-2 text-sm text-muted-foreground hover:bg-foreground/5 hover:text-foreground";
      more.textContent = "Load more directories";
      more.addEventListener("click", () => {
        more.remove();
        void loadDirectory(listing.path, listing.nextCursor, true);
      });
      directoryList.append(more);
    }
  } catch (error) {
    directoryList.replaceChildren();
    setPickerError(error.message);
  }
}

document.querySelectorAll("[data-open-picker]").forEach((button) => {
  button.addEventListener("click", () => {
    picker.classList.remove("hidden");
    picker.classList.add("grid");
    void loadDirectory();
  });
});
document.querySelector("[data-close-picker]")?.addEventListener("click", () => {
  picker.classList.add("hidden");
  picker.classList.remove("grid");
});
parentButton?.addEventListener("click", () => {
  if (parentDirectory) void loadDirectory(parentDirectory);
});
document.querySelector("[data-open-directory]")?.addEventListener("click", () => {
  if (currentDirectory) location.assign(base + "/new?cwd=" + encodeURIComponent(currentDirectory));
});
picker?.addEventListener("click", (event) => {
  if (event.target === picker) document.querySelector("[data-close-picker]")?.click();
});

document.querySelector("[data-device-login]")?.addEventListener("click", async (event) => {
  const button = event.currentTarget;
  const resultNode = document.querySelector("[data-device-result]");
  button.disabled = true;
  button.textContent = "Starting…";
  try {
    const result = await rpc("account/login/start", { type: "chatgptDeviceCode" });
    resultNode.classList.remove("hidden");
    resultNode.replaceChildren();
    const code = document.createElement("strong");
    code.className = "block font-mono text-lg tracking-widest";
    code.textContent = result.userCode;
    const link = document.createElement("a");
    link.className = "mt-2 inline-block text-sm underline underline-offset-4";
    link.href = result.verificationUrl;
    link.target = "_blank";
    link.rel = "noreferrer";
    link.textContent = "Open ChatGPT and enter this code";
    resultNode.append(code, link);
    window.open(result.verificationUrl, "_blank", "noopener,noreferrer");
    const poll = setInterval(async () => {
      try {
        const account = await rpc("account/read", {});
        if (account.account) {
          clearInterval(poll);
          location.reload();
        }
      } catch {}
    }, 1500);
  } catch (error) {
    resultNode.classList.remove("hidden");
    resultNode.textContent = error.message;
  } finally {
    button.disabled = false;
    button.textContent = "Continue with ChatGPT";
  }
});

document.querySelector("[data-api-key-form]")?.addEventListener("submit", async (event) => {
  event.preventDefault();
  const input = event.currentTarget.elements.apiKey;
  const errorNode = document.querySelector("[data-login-error]");
  try {
    await rpc("account/login/start", { type: "apiKey", apiKey: input.value });
    input.value = "";
    location.reload();
  } catch (error) {
    input.value = "";
    errorNode.textContent = error.message;
    errorNode.classList.remove("hidden");
  }
});
