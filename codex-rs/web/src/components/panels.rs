use serde_json::Value;
use topcoat::Result;
use topcoat::view::component;
use topcoat::view::view;

use crate::daemon_config::AccountProfile;
use crate::daemon_config::AccountRole;

#[component]
pub(crate) async fn goal_modal() -> Result {
    view! {
        <div id="goal-modal" class="fixed inset-0 z-[70] hidden place-items-center bg-background/75 p-4 backdrop-blur-sm">
            <section role="dialog" aria-modal="true" aria-labelledby="goal-modal-title" class="w-full max-w-xl overflow-hidden rounded-2xl border border-border bg-background shadow-sm">
                <header class="flex items-start gap-3 border-b border-border px-5 py-4">
                    <div class="min-w-0 flex-1">
                        <h2 id="goal-modal-title" class="text-sm font-semibold">"Session goal"</h2>
                        <p class="mt-1 text-xs text-muted-foreground">"Keep a persistent objective, lifecycle state, and optional token budget for this conversation."</p>
                    </div>
                    <button type="button" data-close-modal="goal-modal" class="grid size-8 place-items-center rounded-lg text-muted-foreground hover:bg-foreground/5 hover:text-foreground" aria-label="Close">"×"</button>
                </header>
                <form id="goal-form" class="grid gap-4 p-5">
                    <label class="grid gap-1.5 text-xs font-medium">
                        "Objective"
                        <textarea id="goal-objective" rows="4" required="" class="resize-y rounded-lg border border-border bg-background px-3 py-2 text-sm leading-6 outline-none focus-visible:ring-2 focus-visible:ring-ring" placeholder="What should Codex keep working toward?"></textarea>
                    </label>
                    <div class="grid grid-cols-2 gap-4 max-sm:grid-cols-1">
                        <label class="grid gap-1.5 text-xs font-medium">
                            "Status"
                            <select id="goal-status" class="h-10 rounded-lg border border-border bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring">
                                <option value="active">"Active"</option>
                                <option value="paused">"Paused"</option>
                                <option value="blocked">"Blocked"</option>
                                <option value="usageLimited">"Usage limited"</option>
                                <option value="budgetLimited">"Budget limited"</option>
                                <option value="complete">"Complete"</option>
                            </select>
                        </label>
                        <label class="grid gap-1.5 text-xs font-medium">
                            "Token budget"
                            <input id="goal-token-budget" type="number" min="1" step="1" class="h-10 rounded-lg border border-border bg-background px-3 font-mono text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring" placeholder="Unlimited">
                        </label>
                    </div>
                    <div id="goal-usage" class="hidden grid-cols-2 gap-3 rounded-xl border border-border bg-foreground/[0.025] p-4 text-xs text-muted-foreground"></div>
                    <p id="goal-error" class="hidden text-xs text-destructive"></p>
                    <footer class="flex items-center gap-2 border-t border-border pt-4">
                        <button id="goal-clear" type="button" class="rounded-lg px-3 py-2 text-sm text-destructive hover:bg-destructive/10">"Clear goal"</button>
                        <button type="button" data-close-modal="goal-modal" class="ml-auto rounded-lg border border-border px-3 py-2 text-sm hover:bg-foreground/5">"Cancel"</button>
                        <button type="submit" class="rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground">"Save goal"</button>
                    </footer>
                </form>
            </section>
        </div>
    }
}

#[component]
pub(crate) async fn settings_modal(
    mcp_servers: &[Value],
    accounts: &[AccountProfile],
    active_account: &str,
) -> Result {
    view! {
        <div id="settings-modal" class="fixed inset-0 z-[70] hidden place-items-center bg-background/75 p-4 backdrop-blur-sm">
            <section role="dialog" aria-modal="true" aria-labelledby="settings-modal-title" class="flex max-h-[min(48rem,92vh)] w-full max-w-2xl flex-col overflow-hidden rounded-2xl border border-border bg-background shadow-sm">
                <header class="flex items-start gap-3 border-b border-border px-5 py-4">
                    <div class="min-w-0 flex-1">
                        <h2 id="settings-modal-title" class="text-sm font-semibold">"Settings"</h2>
                        <p class="mt-1 text-xs text-muted-foreground">"Access, networking, accounts, and MCP connections."</p>
                    </div>
                    <button type="button" data-close-modal="settings-modal" class="grid size-8 place-items-center rounded-lg text-muted-foreground hover:bg-foreground/5 hover:text-foreground" aria-label="Close">"×"</button>
                </header>
                <div class="min-h-0 overflow-y-auto p-5">
                    <section>
                        <h3 class="text-sm font-semibold">"Appearance"</h3>
                        <label class="mt-3 flex items-center justify-between gap-4 rounded-xl border border-border p-4 text-sm">
                            <span><strong class="block font-medium">"Theme"</strong><span class="mt-1 block text-xs text-muted-foreground">"Dark is used by default, including while pages load."</span></span>
                            <select id="settings-theme" class="h-9 rounded-lg border border-border bg-background px-3 text-sm">
                                <option value="dark">"Dark"</option>
                                <option value="light">"Light"</option>
                                <option value="system">"System"</option>
                            </select>
                        </label>
                    </section>
                    <section class="mt-7">
                        <h3 class="text-sm font-semibold">"Private access"</h3>
                        <div class="mt-3 flex items-center gap-4 rounded-xl border border-border p-4 max-sm:items-start">
                            <span class="grid size-8 shrink-0 place-items-center rounded-lg bg-foreground/5" aria-hidden="true">"↗"</span>
                            <span class="min-w-0 flex-1"><strong class="block text-sm font-medium">"Share this Codex Web server"</strong><span class="mt-1 block text-xs leading-5 text-muted-foreground">"The private link grants access to Codex credentials and every filesystem path available to this server user."</span></span>
                            <button type="button" data-copy-private-link="" class="shrink-0 rounded-lg border border-border px-3 py-2 text-xs font-medium hover:bg-foreground/5">"Copy private link"</button>
                        </div>
                        <p data-access-message="" class="mt-3 hidden text-xs text-muted-foreground"></p>
                    </section>
                    <section class="mt-7">
                        <div class="flex items-center gap-2"><h3 class="text-sm font-semibold">"Managed SSH tunnel"</h3><span data-ssh-state="" class="rounded-full bg-foreground/5 px-2 py-1 font-mono text-[0.65rem] text-muted-foreground">"Checking…"</span></div>
                        <p class="mt-1 text-xs leading-5 text-muted-foreground">"Routes selected accounts through a local SOCKS5 proxy owned by this daemon: Codex Web → SSH host → OpenAI."</p>
                        <form data-ssh-tunnel-form="" class="mt-3 grid gap-3 rounded-xl border border-border p-4">
                            <label class="grid gap-1 text-xs font-medium">"SSH destination"<input name="destination" required="" class="h-9 rounded-lg border border-border bg-background px-3 font-mono text-sm" placeholder="user@bastion.example.com"></label>
                            <label class="grid gap-1 text-xs font-medium">"Identity file (optional)"<input name="identityFile" class="h-9 rounded-lg border border-border bg-background px-3 font-mono text-sm" placeholder="/home/me/.ssh/id_ed25519"></label>
                            <div class="grid grid-cols-2 gap-3 max-sm:grid-cols-1">
                                <label class="grid gap-1 text-xs font-medium">"SSH port"<input name="sshPort" type="number" min="1" max="65535" class="h-9 rounded-lg border border-border bg-background px-3 text-sm" placeholder="22"></label>
                                <label class="grid gap-1 text-xs font-medium">"Local SOCKS port"<input name="localPort" type="number" min="1" max="65535" class="h-9 rounded-lg border border-border bg-background px-3 text-sm" placeholder="Automatic"></label>
                            </div>
                            <p class="text-[0.68rem] leading-5 text-muted-foreground">"SSH must work without a password prompt through your agent or key, and the host must already be trusted in known_hosts."</p>
                            <div class="flex flex-wrap items-center gap-2">
                                <button type="button" data-ssh-test="" class="rounded-lg border border-border px-3 py-2 text-sm hover:bg-foreground/5">"Test connection"</button>
                                <button type="submit" class="rounded-lg bg-primary px-3 py-2 text-sm font-medium text-primary-foreground">"Save and apply"</button>
                                <button type="button" data-ssh-disable="" class="ml-auto rounded-lg px-3 py-2 text-sm text-destructive hover:bg-destructive/10">"Disable"</button>
                            </div>
                            <p data-ssh-message="" class="hidden text-xs text-muted-foreground"></p>
                        </form>
                    </section>
                    <section class="mt-7">
                        <h3 class="text-sm font-semibold">"Accounts"</h3>
                        <p class="mt-1 text-xs leading-5 text-muted-foreground">"The primary CLI account stays compatible with normal Codex. Private web fallbacks are tried in this order before a turn starts."</p>
                        <div class="mt-3 divide-y divide-border overflow-hidden rounded-xl border border-border">
                            for account in accounts {
                                <div class="flex items-center gap-3 px-4 py-3 max-sm:flex-wrap">
                                    <span class="grid size-7 place-items-center rounded-lg bg-foreground/5 font-mono text-xs">"@"</span>
                                    <span class="min-w-0 flex-1"><strong class="block truncate text-sm font-medium">(account.label.as_str())</strong><span data-account-status=(account.name.as_str()) class="block truncate font-mono text-[0.66rem] text-muted-foreground">(if account.role == AccountRole::Primary { "Primary Codex account" } else { "Web fallback account" })</span></span>
                                    if account.name == active_account {
                                        <span class="rounded-full bg-sky-500/10 px-2 py-1 text-[0.65rem] font-medium text-sky-400">"Active"</span>
                                    } else {
                                        <button type="button" data-account-use=(account.name.as_str()) class="rounded-lg border border-border px-2 py-1 text-xs hover:bg-foreground/5">"Use"</button>
                                    }
                                    <span class="flex items-center gap-1 max-sm:w-full max-sm:justify-end">
                                        <button type="button" data-account-login=(account.name.as_str()) class="rounded-md px-2 py-1 text-xs text-muted-foreground hover:bg-foreground/5 hover:text-foreground" title="Sign in with ChatGPT">"Login"</button>
                                        <button type="button" data-account-api-key=(account.name.as_str()) class="rounded-md px-2 py-1 text-xs text-muted-foreground hover:bg-foreground/5 hover:text-foreground" title="Use API key">"Key"</button>
                                        <button type="button" data-account-edit=(account.name.as_str()) data-label=(account.label.as_str()) data-proxy=(account.proxy.as_deref().unwrap_or_default()) data-connection=(account_connection(account)) class="grid size-7 place-items-center rounded-md text-muted-foreground hover:bg-foreground/5 hover:text-foreground" title="Edit profile">"⋯"</button>
                                        if account.role == AccountRole::Fallback {
                                            <button type="button" data-account-move=(account.name.as_str()) data-direction="up" class="grid size-7 place-items-center rounded-md text-muted-foreground hover:bg-foreground/5 hover:text-foreground" title="Move earlier">"↑"</button>
                                            <button type="button" data-account-move=(account.name.as_str()) data-direction="down" class="grid size-7 place-items-center rounded-md text-muted-foreground hover:bg-foreground/5 hover:text-foreground" title="Move later">"↓"</button>
                                            <button type="button" data-account-remove=(account.name.as_str()) class="grid size-7 place-items-center rounded-md text-destructive hover:bg-destructive/10" title="Remove profile">"×"</button>
                                        }
                                    </span>
                                </div>
                            }
                        </div>
                        <p data-account-message="" class="mt-3 hidden text-xs text-muted-foreground"></p>
                        <details class="mt-4 rounded-xl border border-border p-4">
                            <summary class="cursor-pointer text-sm font-medium">"Add account profile"</summary>
                            <form data-account-profile-form="" class="mt-4 grid gap-3">
                                <div class="grid grid-cols-2 gap-3 max-sm:grid-cols-1">
                                    <label class="grid gap-1 text-xs font-medium">"Name"<input name="name" required="" pattern="[A-Za-z0-9_-]+" class="h-9 rounded-lg border border-border bg-background px-3 font-mono text-sm" placeholder="personal"></label>
                                    <label class="grid gap-1 text-xs font-medium">"Label"<input name="label" class="h-9 rounded-lg border border-border bg-background px-3 text-sm" placeholder="Personal"></label>
                                </div>
                                <label class="grid gap-1 text-xs font-medium">"Connection"<select name="connection" class="h-9 rounded-lg border border-border bg-background px-3 text-sm"><option value="direct">"Direct"</option><option value="proxy">"Custom HTTP or SOCKS proxy"</option><option value="tunnel">"Managed SSH tunnel"</option></select></label>
                                <label data-account-proxy-field="" class="hidden gap-1 text-xs font-medium">"Proxy URL"<input name="proxy" class="h-9 rounded-lg border border-border bg-background px-3 font-mono text-sm" placeholder="socks5h://127.0.0.1:1080"></label>
                                <button type="submit" class="w-fit rounded-lg border border-border px-3 py-2 text-sm font-medium hover:bg-foreground/5">"Add profile"</button>
                            </form>
                        </details>
                        <p class="mt-3 text-[0.68rem] leading-5 text-muted-foreground">"Account changes take effect immediately. Switching is disabled while a turn is running."</p>
                    </section>
                    <section class="mt-7">
                        <div class="flex items-center gap-2"><h3 class="text-sm font-semibold">"MCP servers"</h3><span class="font-mono text-[0.66rem] text-muted-foreground">(mcp_servers.len())" configured"</span><button type="button" data-mcp-add="" class="ml-auto rounded-lg border border-border px-3 py-1.5 text-xs hover:bg-foreground/5">"Add server"</button></div>
                        <div class="mt-3 divide-y divide-border overflow-hidden rounded-xl border border-border">
                            if mcp_servers.is_empty() {
                                <p class="px-4 py-5 text-sm text-muted-foreground">"No MCP servers are configured."</p>
                            }
                            for server in mcp_servers {
                                <div class="flex items-center gap-3 px-4 py-3">
                                    <span class=(if needs_login(server) { "size-2 rounded-full bg-amber-500" } else { "size-2 rounded-full bg-emerald-500" })></span>
                                    <span class="min-w-0 flex-1"><strong class="block truncate text-sm font-medium">(field(server, "name"))</strong><span class="block text-xs text-muted-foreground">(if needs_login(server) { "Authentication required" } else { "Connected" })</span></span>
                                    if needs_login(server) {
                                        <button type="button" data-mcp-login=(field(server, "name")) class="rounded-lg border border-border px-3 py-1.5 text-xs hover:bg-foreground/5">"Log in"</button>
                                    }
                                    <button type="button" data-mcp-edit=(field(server, "name")) class="rounded-lg border border-border px-3 py-1.5 text-xs hover:bg-foreground/5">"Edit"</button>
                                </div>
                            }
                        </div>
                        <p data-mcp-message="" class="mt-3 hidden text-xs text-muted-foreground"></p>
                    </section>
                    <form data-account-edit-form="" class="mt-7 hidden gap-3 rounded-xl border border-border p-4">
                        <input name="name" type="hidden">
                        <div class="flex items-center"><h3 class="text-sm font-semibold">"Edit account"</h3><button type="button" data-account-edit-close="" class="ml-auto text-muted-foreground">"×"</button></div>
                        <label class="grid gap-1 text-xs font-medium">"Label"<input name="label" required="" class="h-9 rounded-lg border border-border bg-background px-3 text-sm"></label>
                        <label class="grid gap-1 text-xs font-medium">"Connection"<select name="connection" class="h-9 rounded-lg border border-border bg-background px-3 text-sm"><option value="direct">"Direct"</option><option value="proxy">"Custom HTTP or SOCKS proxy"</option><option value="tunnel">"Managed SSH tunnel"</option></select></label>
                        <label data-account-proxy-field="" class="hidden gap-1 text-xs font-medium">"Proxy URL"<input name="proxy" class="h-9 rounded-lg border border-border bg-background px-3 font-mono text-sm" placeholder="http://proxy:8080 or socks5h://127.0.0.1:1080"></label>
                        <div class="flex gap-2"><button type="submit" class="rounded-lg bg-primary px-3 py-2 text-sm font-medium text-primary-foreground">"Save"</button><button type="button" data-account-logout-edit="" class="rounded-lg px-3 py-2 text-sm text-destructive hover:bg-destructive/10">"Sign out"</button></div>
                    </form>
                    <form data-mcp-form="" class="mt-7 hidden gap-3 rounded-xl border border-border p-4">
                        <div class="flex items-center"><h3 class="text-sm font-semibold">"MCP server editor"</h3><button type="button" data-mcp-close="" class="ml-auto text-muted-foreground">"×"</button></div>
                        <input name="originalName" type="hidden">
                        <div class="grid grid-cols-2 gap-3 max-sm:grid-cols-1">
                            <label class="grid gap-1 text-xs font-medium">"Name"<input name="name" required="" pattern="[A-Za-z0-9_-]+" class="h-9 rounded-lg border border-border bg-background px-3 font-mono text-sm"></label>
                            <label class="grid gap-1 text-xs font-medium">"Transport"<select name="transport" class="h-9 rounded-lg border border-border bg-background px-3 text-sm"><option value="stdio">"Local command (stdio)"</option><option value="http">"Streamable HTTP"</option></select></label>
                        </div>
                        <label data-mcp-stdio="" class="grid gap-1 text-xs font-medium">"Command"<input name="command" class="h-9 rounded-lg border border-border bg-background px-3 font-mono text-sm" placeholder="npx"></label>
                        <label data-mcp-stdio="" class="grid gap-1 text-xs font-medium">"Arguments (one per line)"<textarea name="args" rows="3" class="rounded-lg border border-border bg-background px-3 py-2 font-mono text-xs"></textarea></label>
                        <label data-mcp-stdio="" class="grid gap-1 text-xs font-medium">"Working directory"<input name="cwd" class="h-9 rounded-lg border border-border bg-background px-3 font-mono text-sm"></label>
                        <label data-mcp-http="" class="hidden gap-1 text-xs font-medium">"URL"<input name="url" class="h-9 rounded-lg border border-border bg-background px-3 font-mono text-sm" placeholder="https://example.com/mcp"></label>
                        <label data-mcp-http="" class="hidden gap-1 text-xs font-medium">"Bearer token environment variable"<input name="bearerTokenEnvVar" class="h-9 rounded-lg border border-border bg-background px-3 font-mono text-sm"></label>
                        <div class="grid grid-cols-2 gap-3 max-sm:grid-cols-1">
                            <label class="grid gap-1 text-xs font-medium">"Environment / headers (JSON object)"<textarea name="env" rows="4" class="rounded-lg border border-border bg-background px-3 py-2 font-mono text-xs" placeholder="{}"></textarea></label>
                            <label class="grid gap-1 text-xs font-medium">"Environment-backed headers (JSON object)"<textarea name="envHttpHeaders" rows="4" class="rounded-lg border border-border bg-background px-3 py-2 font-mono text-xs" placeholder="{}"></textarea></label>
                        </div>
                        <div class="grid grid-cols-3 gap-3 max-sm:grid-cols-1">
                            <label class="grid gap-1 text-xs font-medium">"Startup timeout (seconds)"<input name="startupTimeoutSec" type="number" min="0" step="0.1" class="h-9 rounded-lg border border-border bg-background px-3 text-sm"></label>
                            <label class="grid gap-1 text-xs font-medium">"Tool timeout (seconds)"<input name="toolTimeoutSec" type="number" min="0" step="0.1" class="h-9 rounded-lg border border-border bg-background px-3 text-sm"></label>
                            <label class="flex items-end gap-2 pb-2 text-xs"><input name="enabled" type="checkbox" checked="">"Enabled"</label>
                        </div>
                        <div class="grid grid-cols-3 gap-3 max-sm:grid-cols-1">
                            <label class="grid gap-1 text-xs font-medium">"Environment ID"<input name="environmentId" class="h-9 rounded-lg border border-border bg-background px-3 font-mono text-sm" placeholder="local"></label>
                            <label class="grid gap-1 text-xs font-medium">"Authentication"<select name="auth" class="h-9 rounded-lg border border-border bg-background px-3 text-sm"><option value="oauth">"OAuth"</option><option value="chatgpt">"ChatGPT session"</option></select></label>
                            <label class="grid gap-1 text-xs font-medium">"Default tool approval"<select name="defaultToolsApprovalMode" class="h-9 rounded-lg border border-border bg-background px-3 text-sm"><option value="">"Default"</option><option value="auto">"Auto"</option><option value="prompt">"Prompt"</option><option value="writes">"Prompt for writes"</option><option value="approve">"Always require approval"</option></select></label>
                        </div>
                        <div class="flex flex-wrap gap-5 text-xs"><label class="flex items-center gap-2"><input name="required" type="checkbox">"Required"</label><label class="flex items-center gap-2"><input name="supportsParallelToolCalls" type="checkbox">"All tools support parallel calls"</label></div>
                        <div class="grid grid-cols-2 gap-3 max-sm:grid-cols-1">
                            <label class="grid gap-1 text-xs font-medium">"Enabled tools (one per line)"<textarea name="enabledTools" rows="3" class="rounded-lg border border-border bg-background px-3 py-2 font-mono text-xs"></textarea></label>
                            <label class="grid gap-1 text-xs font-medium">"Disabled tools (one per line)"<textarea name="disabledTools" rows="3" class="rounded-lg border border-border bg-background px-3 py-2 font-mono text-xs"></textarea></label>
                        </div>
                        <div class="grid grid-cols-2 gap-3 max-sm:grid-cols-1">
                            <label class="grid gap-1 text-xs font-medium">"OAuth scopes (one per line)"<textarea name="scopes" rows="3" class="rounded-lg border border-border bg-background px-3 py-2 font-mono text-xs"></textarea></label>
                            <label class="grid gap-1 text-xs font-medium">"OAuth resource"<input name="oauthResource" class="h-9 rounded-lg border border-border bg-background px-3 font-mono text-sm"></label>
                        </div>
                        <div class="grid grid-cols-3 gap-3 max-sm:grid-cols-1">
                            <label class="grid gap-1 text-xs font-medium">"Environment variables (JSON array)"<textarea name="envVars" rows="4" class="rounded-lg border border-border bg-background px-3 py-2 font-mono text-xs" placeholder="[]"></textarea></label>
                            <label class="grid gap-1 text-xs font-medium">"OAuth client settings (JSON object)"<textarea name="oauth" rows="4" class="rounded-lg border border-border bg-background px-3 py-2 font-mono text-xs" placeholder="{}"></textarea></label>
                            <label class="grid gap-1 text-xs font-medium">"Per-tool settings (JSON object)"<textarea name="tools" rows="4" class="rounded-lg border border-border bg-background px-3 py-2 font-mono text-xs" placeholder="{}"></textarea></label>
                        </div>
                        <div class="flex flex-wrap gap-2"><button type="submit" class="rounded-lg bg-primary px-3 py-2 text-sm font-medium text-primary-foreground">"Save and reload"</button><button type="button" data-mcp-logout="" class="rounded-lg border border-border px-3 py-2 text-sm hover:bg-foreground/5">"Clear OAuth"</button><button type="button" data-mcp-remove="" class="rounded-lg px-3 py-2 text-sm text-destructive hover:bg-destructive/10">"Remove"</button></div>
                        <p class="text-[0.68rem] leading-5 text-muted-foreground">"User-owned servers are editable. Managed or plugin-owned values remain read-only and are identified when saving is rejected."</p>
                    </form>
                </div>
            </section>
        </div>
    }
}

fn field<'a>(value: &'a Value, name: &str) -> &'a str {
    value.get(name).and_then(Value::as_str).unwrap_or_default()
}

fn needs_login(server: &Value) -> bool {
    matches!(field(server, "authStatus"), "notAuthenticated" | "expired")
}

fn account_connection(account: &AccountProfile) -> &'static str {
    if account.use_ssh_tunnel {
        "tunnel"
    } else if account.proxy.is_some() {
        "proxy"
    } else {
        "direct"
    }
}

#[cfg(test)]
#[path = "panels_tests.rs"]
mod tests;
