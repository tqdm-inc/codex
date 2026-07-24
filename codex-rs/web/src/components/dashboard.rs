use http::HeaderValue;
use http::header::CONTENT_SECURITY_POLICY;
use serde_json::Value;
use topcoat::Result;
use topcoat::context::Cx;
use topcoat::view::component;
use topcoat::view::view;

use crate::daemon_config::AccountProfile;
use crate::dashboard::Project;

use super::panels::settings_modal;
use super::views::thread_title;

pub(crate) struct DashboardDocumentData<'a> {
    pub(crate) base_path: &'a str,
    pub(crate) projects: &'a [Project],
    pub(crate) threads: &'a [Value],
    pub(crate) account: Option<&'a Value>,
    pub(crate) truncated: bool,
    pub(crate) profile_label: &'a str,
    pub(crate) proxy_active: bool,
    pub(crate) mcp_servers: &'a [Value],
    pub(crate) accounts: &'a [AccountProfile],
    pub(crate) active_account: &'a str,
}

pub(crate) async fn dashboard_document(cx: &Cx, data: DashboardDocumentData<'_>) -> Result {
    let account_label = data.account.map(account_label).unwrap_or("Not signed in");
    view! { cx =>
        ((CONTENT_SECURITY_POLICY, HeaderValue::from_static(
            "default-src 'self'; img-src 'self' data: blob:; script-src 'self'; style-src 'self'; connect-src 'self'; form-action 'self'"
        )))
        <!DOCTYPE html>
        <html lang="en" class="dark">
            <head>
                <meta charset="utf-8">
                <meta name="viewport" content="width=device-width, initial-scale=1">
                <meta name="color-scheme" content="dark light">
                <meta name="referrer" content="no-referrer">
                <title>"Codex Web"</title>
                <link rel="stylesheet" href="/assets/app.css">
                <script defer="" src="/assets/settings.js"></script>
                <script defer="" src="/assets/home.js"></script>
            </head>
            <body data-dashboard="" data-base-path=(data.base_path)>
                <header class="sticky top-0 z-40 border-b border-border bg-background/90 backdrop-blur">
                    <div class="mx-auto flex h-16 max-w-6xl items-center gap-3 px-6 max-sm:px-4">
                        <span class="grid size-8 place-items-center rounded-lg bg-primary font-mono text-xs font-bold text-primary-foreground">">_"</span>
                        <strong class="text-sm">"Codex Web"</strong>
                        <div class="ml-auto flex min-w-0 items-center gap-2 text-xs text-muted-foreground">
                            if data.proxy_active {
                                <span class="rounded-full border border-sky-500/30 bg-sky-500/10 px-2 py-0.5 font-mono text-[0.62rem] text-sky-600 dark:text-sky-400">"PROXY"</span>
                            }
                            <span class="max-w-32 truncate font-medium text-foreground">(data.profile_label)</span>
                            <span class="max-w-56 truncate max-sm:hidden">(account_label)</span>
                            <button type="button" id="settings-button" class="ml-2 grid size-9 place-items-center rounded-lg border border-border bg-background text-base hover:bg-foreground/5" aria-label="Settings" title="Settings">"⚙"</button>
                        </div>
                    </div>
                </header>
                <main class="mx-auto w-full max-w-6xl px-6 py-12 max-sm:px-4 max-sm:py-8">
                    <section class="flex items-end gap-8 max-md:flex-col max-md:items-start">
                        <div class="min-w-0 flex-1">
                            <p class="font-mono text-[0.68rem] font-semibold uppercase tracking-[0.18em] text-sky-600 dark:text-sky-400">"Workspace ledger"</p>
                            <h1 class="mt-3 max-w-3xl text-4xl font-semibold tracking-[-0.035em] max-sm:text-3xl">"Where should Codex work?"</h1>
                            <p class="mt-3 max-w-2xl text-sm leading-6 text-muted-foreground">"Resume a recent directory or open another folder on this server."</p>
                        </div>
                        <button type="button" data-open-picker="" class="shrink-0 rounded-lg bg-primary px-4 py-2.5 text-sm font-medium text-primary-foreground shadow-xs hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring max-sm:w-full">"Open a folder"</button>
                    </section>

                    <section class="sticky top-16 z-30 -mx-2 mt-8 rounded-xl border border-border bg-background/95 p-2 shadow-xs backdrop-blur">
                        <div class="flex items-center gap-2">
                            <span class="pl-2 text-sm text-muted-foreground" aria-hidden="true">"⌕"</span>
                            <input
                                id="dashboard-search"
                                type="search"
                                autocomplete="off"
                                class="h-10 min-w-0 flex-1 bg-transparent px-2 text-sm outline-none placeholder:text-muted-foreground"
                                placeholder="Search projects, sessions, or paths"
                                aria-label="Search projects and sessions"
                            >
                            <span data-dashboard-results="" aria-live="polite" class="shrink-0 font-mono text-[0.65rem] text-muted-foreground"></span>
                            <button type="button" data-clear-dashboard-search="" class="hidden shrink-0 rounded-lg px-3 py-2 text-xs text-muted-foreground hover:bg-foreground/5 hover:text-foreground">"Clear"</button>
                        </div>
                    </section>

                    if data.account.is_none() {
                        <section class="mt-10 grid gap-6 rounded-2xl border border-border bg-foreground/[0.02] p-6 md:grid-cols-[minmax(0,1fr)_minmax(18rem,0.72fr)]">
                            <div>
                                <p class="font-mono text-[0.68rem] uppercase tracking-[0.16em] text-muted-foreground">"Account required"</p>
                                <h2 class="mt-2 text-xl font-semibold">"Connect Codex before starting work"</h2>
                                <p class="mt-2 text-sm leading-6 text-muted-foreground">"Device login works when this page and the server are on different machines."</p>
                                <button type="button" data-device-login="" class="mt-5 rounded-lg bg-primary px-4 py-2.5 text-sm font-medium text-primary-foreground">"Continue with ChatGPT"</button>
                                <div data-device-result="" class="mt-4 hidden rounded-lg border border-border bg-background p-4 text-sm"></div>
                            </div>
                            <form data-api-key-form="" class="border-border md:border-l md:pl-6">
                                <label for="api-key" class="text-sm font-medium">"Or use an API key"</label>
                                <p class="mt-1 text-xs leading-5 text-muted-foreground">"The key is sent directly to Codex and is never stored by this page."</p>
                                <input id="api-key" name="apiKey" type="password" autocomplete="off" required="" class="mt-4 h-10 w-full rounded-lg border border-border bg-background px-3 font-mono text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring" placeholder="sk-…">
                                <button type="submit" class="mt-3 rounded-lg border border-border bg-background px-3 py-2 text-sm font-medium shadow-xs hover:bg-foreground/5">"Use API key"</button>
                                <p data-login-error="" class="mt-3 hidden text-xs text-destructive"></p>
                            </form>
                        </section>
                    }

                    <section id="recent-projects" class="mt-12">
                        <div class="flex items-baseline gap-3">
                            <h2 class="text-sm font-semibold">"Recent projects"</h2>
                            <span class="font-mono text-[0.68rem] text-muted-foreground">(data.projects.len())" paths"</span>
                        </div>
                        if data.projects.is_empty() {
                            <button type="button" data-open-picker="" class="mt-4 flex w-full items-center justify-between rounded-xl border border-dashed border-border px-5 py-6 text-left hover:bg-foreground/[0.02]">
                                <span>
                                    <strong class="block text-sm">"No recent projects"</strong>
                                    <span class="mt-1 block text-xs text-muted-foreground">"Choose a directory to start the first session."</span>
                                </span>
                                <span class="font-mono text-lg text-muted-foreground">"→"</span>
                            </button>
                        } else {
                            <div data-project-list="" class="mt-4 grid gap-3 md:grid-cols-2 xl:grid-cols-3">
                                for project in data.projects {
                                    project_card(project: project, truncated: data.truncated, base_path: data.base_path)
                                }
                            </div>
                            <button type="button" data-show-more="projects" hidden=(data.projects.len() <= 12) class="mx-auto mt-5 block rounded-lg border border-border px-4 py-2 text-sm text-muted-foreground hover:bg-foreground/5 hover:text-foreground">"Show more projects"</button>
                        }
                    </section>

                    <section id="recent-sessions" class="mt-12">
                        <div class="flex items-baseline gap-3 border-b border-border pb-3">
                            <h2 class="text-sm font-semibold">"Recent sessions"</h2>
                            <span class="font-mono text-[0.68rem] text-muted-foreground">(data.threads.len())(if data.truncated { "+" } else { "" })" loaded"</span>
                        </div>
                        <div data-session-list="">
                            for group in session_groups(data.threads) {
                                <section data-session-group="" class="pt-6">
                                    <h3 class="pb-2 font-mono text-[0.66rem] font-semibold uppercase tracking-[0.14em] text-muted-foreground">(group.label)</h3>
                                    <div class="divide-y divide-border border-t border-border">
                                        for thread in group.threads {
                                            session_row(thread: thread, base_path: data.base_path)
                                        }
                                    </div>
                                </section>
                            }
                        </div>
                        <button type="button" data-show-more="sessions" hidden=(data.threads.len() <= 20) class="mx-auto mt-5 block rounded-lg border border-border px-4 py-2 text-sm text-muted-foreground hover:bg-foreground/5 hover:text-foreground">"Show more sessions"</button>
                    </section>
                    <p data-dashboard-empty="" class="mt-12 hidden rounded-xl border border-dashed border-border px-6 py-10 text-center text-sm text-muted-foreground">"No projects or sessions match this search."</p>
                </main>
                directory_picker()
                settings_modal(mcp_servers: data.mcp_servers, accounts: data.accounts, active_account: data.active_account)
            </body>
        </html>
    }
}

#[component]
async fn project_card(project: &Project, truncated: bool, base_path: &str) -> Result {
    let encoded_cwd = urlencoding::encode(&project.cwd);
    let preview = thread_title(&project.latest_thread);
    let timestamp = recency(&project.latest_thread);
    view! {
        <article
            data-dashboard-item=""
            data-dashboard-kind="project"
            data-search-text=(format!("{} {} {}", project.name, project.cwd, preview).to_lowercase())
            class="group relative overflow-hidden rounded-xl border border-border bg-background p-5 shadow-xs transition-colors hover:border-foreground/25"
        >
            <div class="flex items-start gap-3">
                <span class="mt-0.5 grid size-8 shrink-0 place-items-center rounded-lg border border-border bg-foreground/[0.025] font-mono text-xs text-sky-600 dark:text-sky-400">"⌁"</span>
                <div class="min-w-0 flex-1">
                    <a href=(format!("{base_path}/new?cwd={encoded_cwd}")) class="font-semibold before:absolute before:inset-0">(project.name.as_str())</a>
                    <p class="mt-1 truncate font-mono text-[0.68rem] text-muted-foreground" title=(project.cwd.as_str())>(project.cwd.as_str())</p>
                </div>
            </div>
            <p class="mt-5 truncate text-xs text-muted-foreground">(preview)</p>
            <div class="mt-3 flex items-center gap-2 font-mono text-[0.65rem] text-muted-foreground">
                <span data-relative-time=(timestamp)></span>
                <span>"·"</span>
                <span>(project.session_count)(if truncated { "+" } else { "" })" sessions"</span>
                status_badge(thread: &project.latest_thread)
            </div>
        </article>
    }
}

#[component]
async fn session_row(thread: &Value, base_path: &str) -> Result {
    let id = string(thread, "id");
    let cwd = string(thread, "cwd");
    let timestamp = recency(thread);
    view! {
        <a
            href=(format!("{base_path}/thread/{id}"))
            data-dashboard-item=""
            data-dashboard-kind="session"
            data-search-text=(format!("{} {cwd}", thread_title(thread)).to_lowercase())
            class="group grid grid-cols-[minmax(0,1fr)_auto] gap-x-5 gap-y-1 py-4 hover:bg-foreground/[0.018] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring max-sm:gap-x-3"
        >
            <span class="truncate text-sm font-medium">(thread_title(thread))</span>
            <span class="font-mono text-[0.66rem] text-muted-foreground" data-relative-time=(timestamp)></span>
            <span class="truncate font-mono text-[0.68rem] text-muted-foreground">(cwd)</span>
            status_badge(thread: thread)
        </a>
    }
}

#[component]
async fn status_badge(thread: &Value) -> Result {
    let status = thread
        .get("status")
        .and_then(|status| status.get("type").or(Some(status)))
        .and_then(Value::as_str)
        .unwrap_or("notLoaded");
    let label = match status {
        "active" => "Active",
        "idle" => "Idle",
        "systemError" => "Error",
        _ => "Stored",
    };
    view! {
        <span class=(if status == "active" {
            "ml-auto inline-flex items-center gap-1.5 text-[0.65rem] text-sky-600 dark:text-sky-400"
        } else if status == "systemError" {
            "ml-auto inline-flex items-center gap-1.5 text-[0.65rem] text-destructive"
        } else {
            "ml-auto inline-flex items-center gap-1.5 text-[0.65rem] text-muted-foreground"
        })>
            <span class=(if status == "active" { "size-1.5 rounded-full bg-sky-500" } else { "size-1.5 rounded-full bg-foreground/20" })></span>
            (label)
        </span>
    }
}

#[component]
async fn directory_picker() -> Result {
    view! {
        <div data-picker="" class="fixed inset-0 z-50 hidden place-items-center bg-background/70 p-4 backdrop-blur-sm">
            <section role="dialog" aria-modal="true" aria-labelledby="picker-title" class="flex max-h-[min(44rem,90vh)] w-full max-w-2xl flex-col overflow-hidden rounded-2xl border border-border bg-background shadow-sm">
                <header class="flex items-center gap-3 border-b border-border px-5 py-4">
                    <div class="min-w-0 flex-1">
                        <h2 id="picker-title" class="text-sm font-semibold">"Choose a directory"</h2>
                        <p data-picker-path="" class="mt-1 truncate font-mono text-[0.68rem] text-muted-foreground"></p>
                    </div>
                    <button type="button" data-close-picker="" class="grid size-8 place-items-center rounded-lg text-muted-foreground hover:bg-foreground/5 hover:text-foreground" aria-label="Close">"×"</button>
                </header>
                <nav data-breadcrumbs="" class="flex min-h-11 items-center gap-1 overflow-x-auto border-b border-border px-4 font-mono text-xs"></nav>
                <div data-directory-list="" class="min-h-56 flex-1 overflow-y-auto p-2"></div>
                <p data-picker-error="" class="hidden border-t border-border px-5 py-3 text-xs text-destructive"></p>
                <footer class="flex items-center gap-3 border-t border-border px-5 py-4">
                    <button type="button" data-parent-directory="" class="rounded-lg border border-border px-3 py-2 text-sm hover:bg-foreground/5">"Up"</button>
                    <button type="button" data-open-directory="" class="ml-auto rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground">"Open this folder"</button>
                </footer>
            </section>
        </div>
    }
}

fn account_label(account: &Value) -> &str {
    account
        .get("email")
        .and_then(Value::as_str)
        .unwrap_or_else(|| match string(account, "type") {
            "apiKey" => "API key",
            "chatgpt" => "ChatGPT",
            _ => "Connected",
        })
}

fn recency(thread: &Value) -> i64 {
    thread
        .get("recencyAt")
        .or_else(|| thread.get("updatedAt"))
        .and_then(Value::as_i64)
        .unwrap_or_default()
}

struct SessionGroup<'a> {
    label: &'static str,
    threads: Vec<&'a Value>,
}

fn session_groups(threads: &[Value]) -> Vec<SessionGroup<'_>> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default();
    session_groups_at(threads, now)
}

fn session_groups_at(threads: &[Value], now: i64) -> Vec<SessionGroup<'_>> {
    const BUCKETS: [(&str, i64); 5] = [
        ("Today", 86_400),
        ("Yesterday", 172_800),
        ("Previous 7 days", 604_800),
        ("Previous 30 days", 2_592_000),
        ("Older", i64::MAX),
    ];
    let mut groups = BUCKETS
        .into_iter()
        .map(|(label, _)| SessionGroup {
            label,
            threads: Vec::new(),
        })
        .collect::<Vec<_>>();
    for thread in threads.iter().take(100) {
        let age = now.saturating_sub(recency(thread)).max(0);
        let index = BUCKETS
            .iter()
            .position(|(_, upper_bound)| age < *upper_bound)
            .unwrap_or(BUCKETS.len() - 1);
        groups[index].threads.push(thread);
    }
    groups
        .into_iter()
        .filter(|group| !group.threads.is_empty())
        .collect()
}

fn string<'a>(value: &'a Value, field: &str) -> &'a str {
    value.get(field).and_then(Value::as_str).unwrap_or_default()
}

#[cfg(test)]
#[path = "dashboard_tests.rs"]
mod tests;
