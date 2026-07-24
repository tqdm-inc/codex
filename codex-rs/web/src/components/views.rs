use http::HeaderValue;
use http::header::CONTENT_SECURITY_POLICY;
use serde_json::Value;
use topcoat::Result;
use topcoat::context::Cx;
use topcoat::view::Unescaped;
use topcoat::view::attributes;
use topcoat::view::component;
use topcoat::view::view;

use crate::daemon_config::AccountProfile;

use super::markdown::markdown;
use super::panels::goal_modal;
use super::panels::settings_modal;
use super::ui::badge::BadgeVariant;
use super::ui::badge::badge;
use super::ui::button::ButtonSize;
use super::ui::button::ButtonVariant;
use super::ui::button::button;
use super::ui::card::card;
use super::ui::card::card_content;
use super::ui::card::card_footer;
use super::ui::card::card_header;
use super::ui::card::card_title;
use super::ui::select::select;
use super::ui::switch::switch;
use super::ui::textarea::textarea;
use super::work_digest::activity_row;
use super::work_digest::is_activity;
use super::work_digest::is_visible_activity;
use super::work_digest::work_digest;

const SIDEBAR_LAYOUT: &str = "flex h-dvh max-h-dvh min-h-0 min-w-0 flex-col overflow-hidden \
    border-r border-border bg-foreground/[0.035] p-3 max-md:fixed max-md:inset-y-0 \
    max-md:left-0 max-md:z-50 max-md:w-[min(20rem,86vw)] max-md:-translate-x-full \
    max-md:bg-background max-md:shadow-sm max-md:transition-transform";
const THREAD_LIST_LAYOUT: &str = "min-h-0 flex-1 overflow-y-auto overscroll-contain";
const QUEUE_PANEL_LAYOUT: &str = "pointer-events-auto mb-2 overflow-hidden rounded-xl border \
    border-border bg-background shadow-sm";
const RUN_STATUS_LAYOUT: &str = "mb-2 flex w-fit items-center gap-2 rounded-full border \
    border-border bg-background/95 px-3 py-1.5 text-xs text-muted-foreground shadow-xs";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PlanPresentation {
    #[default]
    Historical,
    Streaming,
    Completed,
    Actionable,
}

pub(crate) struct DocumentData<'a> {
    pub(crate) base_path: &'a str,
    pub(crate) instance_id: &'a str,
    pub(crate) base_seq: u64,
    pub(crate) threads: &'a [Value],
    pub(crate) active_thread: Option<&'a Value>,
    pub(crate) active_model: &'a str,
    pub(crate) active_effort: &'a str,
    pub(crate) active_permission_mode: &'a str,
    pub(crate) models: &'a [Value],
    pub(crate) collaboration_modes: &'a [Value],
    pub(crate) approvals: &'a [Value],
    pub(crate) workspace_cwd: &'a str,
    pub(crate) mcp_servers: &'a [Value],
    pub(crate) accounts: &'a [AccountProfile],
    pub(crate) active_account: &'a str,
    pub(crate) initial_next_cursor: Option<&'a str>,
}

pub(crate) async fn bootstrap_document(cx: &Cx) -> Result {
    view! { cx =>
        ((CONTENT_SECURITY_POLICY, HeaderValue::from_static(
            "default-src 'self'; img-src 'self' data:; script-src 'self'; style-src 'self'; connect-src 'self'"
        )))
        <!DOCTYPE html>
        <html lang="en" class="dark">
            <head>
                <meta charset="utf-8">
                <meta name="viewport" content="width=device-width, initial-scale=1">
                <meta name="color-scheme" content="dark light">
                <meta name="referrer" content="no-referrer">
                <title>"Open Codex Web"</title>
                <link rel="stylesheet" href="/assets/app.css">
                <script defer="" src="/assets/bootstrap.js"></script>
            </head>
            <body>
                <main class="grid min-h-dvh place-items-center px-6">
                    <section class="w-full max-w-lg overflow-hidden rounded-2xl border border-border bg-background shadow-sm">
                        <div class="border-b border-border bg-foreground/[0.025] px-6 py-4 font-mono text-xs text-muted-foreground">
                            <span class="text-sky-500">"~/codex"</span>
                            <span>" / private session"</span>
                        </div>
                        <div class="px-6 py-8">
                            <div class="grid size-11 place-items-center rounded-xl border border-border bg-foreground/[0.025] font-mono text-sm font-semibold">">_"</div>
                            <h1 id="bootstrap-heading" class="mt-5 text-2xl font-semibold tracking-tight">"Checking access"</h1>
                            <p id="bootstrap-status" aria-live="polite" class="mt-2 text-sm leading-6 text-muted-foreground">"Validating the private link…"</p>
                        </div>
                    </section>
                </main>
            </body>
        </html>
    }
}

pub(crate) async fn document(cx: &Cx, data: DocumentData<'_>) -> Result {
    let DocumentData {
        base_path,
        instance_id,
        base_seq,
        threads,
        active_thread,
        active_model,
        active_effort,
        active_permission_mode,
        models,
        collaboration_modes,
        approvals,
        workspace_cwd,
        mcp_servers,
        accounts,
        active_account,
        initial_next_cursor,
    } = data;
    let title = active_thread.map(thread_title).unwrap_or("New task");
    let cwd = active_thread
        .and_then(|thread| thread.get("cwd"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let active_id = active_thread
        .and_then(|thread| thread.get("id"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let active_turn_id = active_thread
        .and_then(|thread| thread.get("turns"))
        .and_then(Value::as_array)
        .and_then(|turns| {
            turns
                .iter()
                .rev()
                .find(|turn| string(turn, "status") == "inProgress")
        })
        .map(|turn| string(turn, "id"))
        .unwrap_or_default();
    view! { cx =>
        ((CONTENT_SECURITY_POLICY, HeaderValue::from_static(
            "default-src 'self'; img-src 'self' data: blob:; script-src 'self'; style-src 'self'; connect-src 'self'"
        )))
        <!DOCTYPE html>
        <html lang="en" class="dark">
            <head>
                <meta charset="utf-8">
                <meta name="viewport" content="width=device-width, initial-scale=1">
                <meta name="color-scheme" content="dark light">
                <title>(title)" - Codex"</title>
                <link rel="stylesheet" href="/assets/app.css">
                <script defer="" src="/assets/settings.js"></script>
                <script defer="" src="/assets/app.js"></script>
            </head>
            <body
                data-base-path=(base_path)
                data-instance-id=(instance_id)
                data-base-seq=(base_seq)
                data-thread-id=(active_id)
                data-active-turn-id=(active_turn_id)
                data-workspace-cwd=(workspace_cwd)
            >
                <div id="app-shell" class="grid h-dvh grid-cols-[17rem_minmax(0,1fr)] overflow-hidden max-md:grid-cols-1">
                    sidebar(threads: threads, active_id: active_id, base_path: base_path)
                    <button id="sidebar-backdrop" type="button" class="fixed inset-0 z-40 hidden bg-black/45 max-md:block max-md:opacity-0 max-md:pointer-events-none" aria-label="Close navigation"></button>
                    <main id="chat-main" class="relative grid h-dvh min-h-0 min-w-0 grid-rows-[auto_minmax(0,1fr)] overflow-hidden bg-background">
                        <div id="session-loading" class="pointer-events-none absolute inset-x-0 top-0 z-50" hidden="" role="status" aria-live="polite">
                            <div class="h-0.5 w-full overflow-hidden bg-foreground/10"><div class="live-progress h-full w-1/3 bg-sky-500"></div></div>
                            <span class="absolute right-4 top-3 rounded-full border border-border bg-background/95 px-3 py-1.5 text-xs text-muted-foreground shadow-sm">"Opening conversation…"</span>
                        </div>
                        app_header(
                            title: title,
                            cwd: cwd,
                            models: models,
                            active_model: active_model,
                            active_effort: active_effort,
                            collaboration_modes: collaboration_modes,
                        )
                        <section
                            id="transcript"
                            class="min-h-0 overflow-y-auto overscroll-contain px-[max(1.25rem,calc((100%-48rem)/2))] pt-8 pb-40"
                            aria-live="polite"
                        >
                            if let Some(cursor) = initial_next_cursor {
                                <div class="mb-6 flex justify-center" data-history-loader="">
                                    <button
                                        type="button"
                                        class="rounded-full border border-border bg-background px-3 py-1.5 text-xs text-muted-foreground shadow-xs hover:text-foreground"
                                        data-load-earlier=(cursor)
                                    >"Load earlier messages"</button>
                                </div>
                            }
                            (transcript_fragment(cx, active_thread, approvals).await?)
                        </section>
                        <button id="new-updates" type="button" class="absolute bottom-40 left-1/2 z-30 -translate-x-1/2 rounded-full border border-border bg-background px-3 py-1.5 text-xs font-medium shadow-sm" hidden="">"New updates"</button>
                        composer(thread_id: active_id, permission_mode: active_permission_mode)
                    </main>
                </div>
                goal_modal()
                settings_modal(mcp_servers: mcp_servers, accounts: accounts, active_account: active_account)
            </body>
        </html>
    }
}

#[component]
async fn sidebar(threads: &[Value], active_id: &str, base_path: &str) -> Result {
    let has_active_threads = threads.iter().any(thread_is_active);
    view! {
        <aside
            id="sidebar"
            class=(SIDEBAR_LAYOUT)
        >
            <div class="flex h-10 shrink-0 items-center gap-2 px-2">
                <a href=(base_path) class="grid size-7 place-items-center rounded-lg bg-primary font-mono text-xs font-bold text-primary-foreground" aria-label="Home">">_"</a>
                <a href=(base_path) data-sidebar-expanded="" class="text-sm font-semibold">"Codex"</a>
                <a
                    href=(format!("{base_path}/new"))
                    data-session-link=""
                    data-thread-id=""
                    class="ml-auto grid size-8 shrink-0 place-items-center rounded-lg text-xl hover:bg-foreground/5"
                    aria-label="New task"
                    title="New task"
                >"+"</a>
                <button id="sidebar-collapse-button" type="button" class="grid size-8 shrink-0 place-items-center rounded-lg text-muted-foreground hover:bg-foreground/5 hover:text-foreground max-md:hidden" aria-controls="sidebar" aria-expanded="true" aria-label="Collapse navigation" title="Collapse navigation">"‹"</button>
                <button id="sidebar-close-button" type="button" class="hidden size-8 shrink-0 place-items-center rounded-lg text-muted-foreground hover:bg-foreground/5 hover:text-foreground max-md:grid" aria-label="Close navigation">"×"</button>
            </div>
            <input
                id="thread-search"
                data-sidebar-expanded=""
                class="mt-3 h-9 shrink-0 rounded-lg border border-border bg-background px-3 text-sm outline-none placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring"
                placeholder="Search chats"
                aria-label="Search chats"
            >
            <label data-sidebar-expanded="" class="flex shrink-0 cursor-pointer items-center gap-2 px-2 py-3 text-xs text-muted-foreground">
                switch(attrs: attributes! {
                    id="cwd-only-toggle"
                    checked=""
                    aria-label="Show sessions for this folder only"
                })
                <span>"This folder only"</span>
            </label>
            <nav id="thread-list" data-sidebar-expanded="" class=(THREAD_LIST_LAYOUT) aria-label="Chats">
                <div id="active-thread-list">
                    <p
                        id="active-thread-heading"
                        class="px-3 pb-1 pt-2 text-[0.65rem] font-medium uppercase tracking-widest text-muted-foreground"
                        hidden=(!has_active_threads)
                    >"Active"</p>
                    for thread in threads.iter().filter(|thread| thread_is_active(thread)) {
                        thread_link(thread: thread, active_id: active_id, base_path: base_path)
                    }
                </div>
                <div id="inactive-thread-list">
                    for thread in threads.iter().filter(|thread| !thread_is_active(thread)) {
                        thread_link(thread: thread, active_id: active_id, base_path: base_path)
                    }
                </div>
            </nav>
            <div class="mt-auto flex shrink-0 items-center gap-2 px-2 py-2 text-xs text-muted-foreground">
                <span id="connection-dot" class="size-2 rounded-full bg-amber-500"></span>
                <span id="connection-status" data-sidebar-expanded="">"Connecting"</span>
                <span id="web-statusline" data-sidebar-expanded="" class="ml-auto truncate font-mono text-[0.65rem]"></span>
            </div>
        </aside>
    }
}

#[component]
async fn thread_link(thread: &Value, active_id: &str, base_path: &str) -> Result {
    let id = string(thread, "id");
    let active = id == active_id;
    view! {
        <a
            href=(format!("{base_path}/thread/{id}"))
            data-session-link=""
            data-thread-id=(id)
            data-thread-title=(thread_title(thread).to_lowercase())
            data-thread-cwd=(string(thread, "cwd"))
            data-thread-active=(thread_is_active(thread).to_string())
            class=(if active {
                "group flex items-center gap-2 rounded-lg bg-foreground/[0.07] px-3 py-2 text-sm hover:bg-foreground/5"
            } else {
                "group flex items-center gap-2 rounded-lg px-3 py-2 text-sm hover:bg-foreground/5"
            })
        >
            <span class="min-w-0 flex-1 truncate">(thread_title(thread))</span>
            <span data-thread-live="" class="hidden size-1.5 shrink-0 rounded-full bg-sky-500"></span>
            <span data-thread-unread="" class="hidden min-w-4 rounded-full bg-primary px-1 text-center text-[0.65rem] text-primary-foreground">"0"</span>
        </a>
    }
}

#[component]
async fn app_header(
    title: &str,
    cwd: &str,
    models: &[Value],
    active_model: &str,
    active_effort: &str,
    collaboration_modes: &[Value],
) -> Result {
    let selected_model = models
        .iter()
        .find(|model| model_matches(model, active_model))
        .or_else(|| models.iter().find(|model| bool_value(model, "isDefault")))
        .or_else(|| models.first());
    let selected_effort = if active_effort.is_empty() {
        selected_model
            .map(|model| string(model, "defaultReasoningEffort"))
            .unwrap_or_default()
    } else {
        active_effort
    };
    view! {
        <header class="border-b border-border bg-background/90 px-5 py-3 backdrop-blur max-md:px-3">
            <div class="flex min-h-10 items-center gap-3">
            button(
                variant: ButtonVariant::Ghost,
                size: ButtonSize::Icon,
                attrs: attributes! {
                    id="menu-button"
                    type="button"
                    class="hidden max-md:inline-flex"
                    aria-label="Open navigation"
                    title="Open navigation"
                },
                "☰"
            )
            <div class="min-w-0 flex-1">
                <h1 id="thread-title" class="truncate text-sm font-semibold">(title)</h1>
                <p id="thread-cwd" class="truncate text-xs text-muted-foreground">(cwd)</p>
            </div>
            <div class="ml-auto flex shrink-0 items-center gap-2 max-sm:gap-1">
                <button id="goal-button" type="button" class="rounded-lg border border-border bg-background px-3 py-2 text-xs font-medium hover:bg-foreground/5 max-sm:px-2">"Goal"</button>
                <button id="settings-button" type="button" class="grid size-9 place-items-center rounded-lg border border-border bg-background text-base hover:bg-foreground/5" aria-label="Settings" title="Settings">"⚙"</button>
            </div>
            <div class="ml-2 flex min-w-0 items-center gap-2 max-md:hidden">
                if !models.is_empty() {
                    select(
                        attrs: attributes! {
                            id="model-select"
                            class="w-40 max-sm:w-28"
                            aria-label="Model"
                        },
                        for model in models {
                            <option
                                value=(string(model, "id"))
                                data-efforts=(model_efforts(model))
                                data-default-effort=(string(model, "defaultReasoningEffort"))
                                selected=(selected_model == Some(model))
                            >(model_label(model))</option>
                        }
                    )
                    select(
                        attrs: attributes! {
                            id="effort-select"
                            class="w-28 max-sm:w-24"
                            aria-label="Reasoning effort"
                        },
                        if let Some(model) = selected_model {
                            for effort in model.get("supportedReasoningEfforts").and_then(Value::as_array).into_iter().flatten() {
                                <option
                                    value=(string(effort, "reasoningEffort"))
                                    selected=(string(effort, "reasoningEffort") == selected_effort)
                                >(effort_label(string(effort, "reasoningEffort")))</option>
                            }
                        }
                    )
                }
                select(
                    attrs: attributes! {
                        id="mode-select"
                        class="w-24 max-sm:w-20"
                        aria-label="Collaboration mode"
                    },
                    if collaboration_modes.is_empty() {
                        <option value="default" selected="">"Default"</option>
                        <option value="plan">"Plan"</option>
                    } else {
                        for mode in collaboration_modes.iter().filter(|mode| string(mode, "mode") == "default") {
                            <option
                                value=(string(mode, "mode"))
                                data-model=(string(mode, "model"))
                                data-effort=(mode_effort(mode))
                                selected=""
                            >(string(mode, "name"))</option>
                        }
                        for mode in collaboration_modes.iter().filter(|mode| string(mode, "mode") != "default") {
                            if !string(mode, "mode").is_empty() {
                                <option
                                    value=(string(mode, "mode"))
                                    data-model=(string(mode, "model"))
                                    data-effort=(mode_effort(mode))
                                >(string(mode, "name"))</option>
                            }
                        }
                    }
                )
            </div>
            </div>
            <div class="mt-2 hidden min-w-0 grid-cols-3 gap-2 max-md:grid">
                if !models.is_empty() {
                    <select id="mobile-model-select" class="h-9 min-w-0 rounded-lg border border-border bg-background px-2 text-xs" aria-label="Model">
                        for model in models {
                            <option value=(string(model, "id")) data-efforts=(model_efforts(model)) data-default-effort=(string(model, "defaultReasoningEffort")) selected=(selected_model == Some(model))>(model_label(model))</option>
                        }
                    </select>
                    <select id="mobile-effort-select" class="h-9 min-w-0 rounded-lg border border-border bg-background px-2 text-xs" aria-label="Reasoning effort">
                        if let Some(model) = selected_model {
                            for effort in model.get("supportedReasoningEfforts").and_then(Value::as_array).into_iter().flatten() {
                                <option value=(string(effort, "reasoningEffort")) selected=(string(effort, "reasoningEffort") == selected_effort)>(effort_label(string(effort, "reasoningEffort")))</option>
                            }
                        }
                    </select>
                }
                <select id="mobile-mode-select" class="h-9 min-w-0 rounded-lg border border-border bg-background px-2 text-xs" aria-label="Collaboration mode">
                    if collaboration_modes.is_empty() {
                        <option value="default" selected="">"Default"</option>
                        <option value="plan">"Plan"</option>
                    } else {
                        for mode in collaboration_modes {
                            if !string(mode, "mode").is_empty() {
                                <option value=(string(mode, "mode")) data-model=(string(mode, "model")) data-effort=(mode_effort(mode)) selected=(string(mode, "mode") == "default")>(string(mode, "name"))</option>
                            }
                        }
                    }
                </select>
            </div>
        </header>
    }
}

#[component]
async fn empty_state() -> Result {
    view! {
        <div id="empty-state" class="grid min-h-[58vh] place-content-center justify-items-center text-center">
            <div class="grid size-12 place-items-center rounded-xl border border-border bg-background font-mono font-bold shadow-xs">">_"</div>
            <h2 class="mt-5 text-3xl font-semibold">"What are we building?"</h2>
        </div>
    }
}

#[component]
async fn composer(thread_id: &str, permission_mode: &str) -> Result {
    view! {
        <footer id="composer-dock" class="pointer-events-none absolute inset-x-0 bottom-0 bg-gradient-to-t from-background via-background to-transparent px-[max(1.25rem,calc((100%-48rem)/2))] pb-5 pt-12">
            <section id="queue-panel" class=(QUEUE_PANEL_LAYOUT) hidden="" aria-label="Queued messages">
                <div class="flex items-center gap-2 border-b border-border px-3 py-2">
                    <strong id="queue-count" class="text-xs">"Queued"</strong>
                    <span id="queue-state" class="text-xs text-muted-foreground"></span>
                    <button id="clear-queue-button" type="button" class="ml-auto text-xs text-muted-foreground hover:text-foreground">"Clear"</button>
                    <button id="run-queue-button" type="button" class="rounded-md bg-primary px-2.5 py-1 text-xs font-medium text-primary-foreground hover:bg-primary/90">"Run next"</button>
                </div>
                <div id="queue-list" class="max-h-36 overflow-y-auto p-1"></div>
            </section>
            <div id="attachments" class="pointer-events-auto mb-2 flex gap-2"></div>
            <form
                id="composer"
                class="relative pointer-events-auto rounded-xl border border-border bg-background p-3 shadow-sm"
                data-thread-id=(thread_id)
            >
                <section id="command-palette" class="absolute inset-x-0 bottom-[calc(100%+0.5rem)] z-40 max-h-72 overflow-y-auto rounded-xl border border-border bg-background p-1 shadow-sm" hidden="" role="listbox" aria-label="Composer suggestions"></section>
                <div id="run-status" class=(RUN_STATUS_LAYOUT) hidden="" role="status" aria-live="polite">
                    <span id="codex-pet" class="hidden" aria-hidden="true">"◖•ᴥ•◗"</span>
                    <span class="relative flex size-2">
                        <span class="absolute inline-flex size-full animate-ping rounded-full bg-emerald-500 opacity-40"></span>
                        <span class="relative inline-flex size-2 rounded-full bg-emerald-500"></span>
                    </span>
                    <span class="live-rail h-4 w-0.5 rounded-full bg-sky-500"></span>
                    <span id="run-status-label">"Starting Codex…"</span>
                    <span id="run-elapsed" class="font-mono text-[0.65rem] tabular-nums"></span>
                </div>
                textarea(
                    attrs: attributes! {
                        id="prompt"
                        name="prompt"
                        rows="1"
                        class="max-h-44 min-h-8 resize-none border-0 p-1 shadow-none focus-visible:ring-0"
                        placeholder="Message Codex"
                        autofocus=""
                    }
                )
                <div class="mt-2 flex min-w-0 items-center gap-2 max-sm:flex-wrap">
                    button(
                        variant: ButtonVariant::Outline,
                        size: ButtonSize::Icon,
                        attrs: attributes! {
                            id="attach-button"
                            type="button"
                            aria-label="Attach images"
                            title="Attach images"
                        },
                        "+"
                    )
                    <input id="image-input" type="file" accept="image/*" multiple="" hidden="">
                    select(
                        attrs: attributes! {
                            id="permissions-select"
                            class="w-28"
                            aria-label="Model permissions"
                            title="Model permissions"
                        },
                        <option value="workspace" selected=(permission_mode == "workspace")>"Workspace"</option>
                        <option value="full-access" selected=(permission_mode == "full-access")>"YOLO"</option>
                        <option value="read-only" selected=(permission_mode == "read-only")>"Read Only"</option>
                    )
                    <label id="composer-plan-control" class="flex cursor-pointer items-center gap-2 rounded-lg px-2 py-1 text-xs text-muted-foreground hover:bg-foreground/5 hover:text-foreground">
                        switch(attrs: attributes! {
                            id="composer-plan-toggle"
                            aria-label="Plan mode"
                        })
                        <span>"Plan"</span>
                    </label>
                    <span id="composer-hint" class="text-xs text-muted-foreground max-sm:hidden">"Shift + Enter for a new line"</span>
                    <div class="ml-auto flex items-center gap-2">
                        button(
                            variant: ButtonVariant::Destructive,
                            size: ButtonSize::Icon,
                            attrs: attributes! {
                                id="stop-button"
                                type="button"
                                aria-label="Stop execution"
                                title="Stop execution"
                                hidden=""
                            },
                            <span id="stop-spinner" class="hidden size-4 animate-spin rounded-full border-2 border-current border-t-transparent"></span>
                            <span id="stop-icon" aria-hidden="true">"■"</span>
                        )
                        button(
                            size: ButtonSize::Icon,
                            attrs: attributes! {
                                id="send-button"
                                type="submit"
                                aria-label="Send message"
                                title="Send message"
                            },
                            <span id="send-spinner" class="hidden size-4 animate-spin rounded-full border-2 border-current border-t-transparent"></span>
                            <span id="send-icon">"↑"</span>
                        )
                    </div>
                </div>
            </form>
        </footer>
    }
}

pub(crate) async fn item_fragment(cx: &Cx, item: &Value) -> Result {
    item_fragment_with_plan_presentation(cx, item, PlanPresentation::Historical).await
}

pub(crate) async fn item_fragment_with_plan_presentation(
    cx: &Cx,
    item: &Value,
    plan_presentation: PlanPresentation,
) -> Result {
    let item_type = string(item, "type");
    let id = string(item, "id");
    match item_type {
        "userMessage" => user_message(cx, id, item).await,
        "agentMessage" => agent_message(cx, id, item).await,
        "plan" => plan_proposal(cx, id, string(item, "text"), plan_presentation).await,
        _ if is_activity(item) => activity_row(cx, item).await,
        _ => activity_card(cx, id, item_type, pretty(Some(item)), item).await,
    }
}

pub(crate) async fn transcript_fragment(
    cx: &Cx,
    active_thread: Option<&Value>,
    approvals: &[Value],
) -> Result {
    let turns = active_thread
        .and_then(|thread| thread.get("turns"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let has_visible_items = turns.iter().any(turn_has_visible_items);
    let actionable_plan_id = active_thread.and_then(actionable_plan_id);
    view! { cx =>
        if !has_visible_items {
            empty_state()
        } else {
            for turn in turns {
                (turn_fragment(cx, &turn, actionable_plan_id).await?)
            }
        }
        for approval in approvals {
            (approval_fragment(cx, approval).await?)
        }
    }
}

async fn turn_fragment(cx: &Cx, turn: &Value, actionable_plan_id: Option<&str>) -> Result {
    let items = turn
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let activities = items
        .iter()
        .filter(|item| is_visible_activity(item))
        .cloned()
        .collect::<Vec<_>>();
    let first_activity_id = activities.first().map(|item| string(item, "id"));
    let turn_id = string(turn, "id");
    let turn_status = string(turn, "status");
    view! { cx =>
        for item in items {
            if first_activity_id == Some(string(&item, "id")) {
                (work_digest(cx, turn_id, turn_status, &activities).await?)
            }
            if !is_activity(&item) {
                (item_fragment_with_plan_presentation(
                    cx,
                    &item,
                    if actionable_plan_id == Some(string(&item, "id")) {
                        PlanPresentation::Actionable
                    } else {
                        PlanPresentation::Historical
                    },
                ).await?)
            }
        }
    }
}

fn turn_has_visible_items(turn: &Value) -> bool {
    turn.get("items")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items
                .iter()
                .any(|item| !is_activity(item) || is_visible_activity(item))
        })
}

async fn plan_proposal(
    cx: &Cx,
    id: &str,
    plan_markdown: &str,
    presentation: PlanPresentation,
) -> Result {
    let rendered_plan = markdown(plan_markdown);
    let completed = matches!(
        presentation,
        PlanPresentation::Historical | PlanPresentation::Completed | PlanPresentation::Actionable
    );
    let actionable = presentation == PlanPresentation::Actionable;
    view! { cx =>
        <article
            id=(format!("item-{id}"))
            class="plan-proposal mb-7 overflow-hidden rounded-xl border border-border bg-foreground/[0.018] shadow-xs"
            data-item-id=(id)
            data-plan-proposal=""
            data-plan-complete=(completed.to_string())
            data-plan-actionable=(actionable.to_string())
            data-plan-markdown=(plan_markdown)
        >
            <header class="flex items-center gap-3 border-b border-border bg-foreground/[0.025] px-4 py-3">
                <span class="grid size-7 shrink-0 place-items-center rounded-lg border border-border bg-background font-mono text-xs font-semibold">(if completed { "✓" } else { "…" })</span>
                <div class="min-w-0">
                    <h2 class="text-sm font-semibold">"Proposed plan"</h2>
                    <p class="text-xs text-muted-foreground">(match presentation {
                        PlanPresentation::Streaming => "Codex is preparing the proposal",
                        PlanPresentation::Actionable => "Ready for your decision",
                        PlanPresentation::Completed => "Finishing the planning turn",
                        PlanPresentation::Historical => "Plan proposal",
                    })</p>
                </div>
                if !completed {
                    <span class="live-rail ml-auto h-4 w-0.5 rounded-full bg-sky-500"></span>
                }
            </header>
            <div class="prose max-w-none px-5 py-5 text-sm leading-7">
                (Unescaped::new_unchecked(rendered_plan))
            </div>
            if matches!(presentation, PlanPresentation::Completed | PlanPresentation::Actionable) {
                <footer
                    class="border-t border-border bg-background px-4 py-4"
                    data-plan-actions=""
                    hidden=(!actionable)
                >
                    <div class="flex items-start gap-3 max-sm:flex-col">
                        <div class="min-w-0 flex-1">
                            <p class="text-sm font-semibold">"Implement this plan?"</p>
                            <p class="mt-1 text-xs text-muted-foreground" data-plan-status="">"Choose how Codex should continue."</p>
                        </div>
                        <div class="flex shrink-0 flex-wrap justify-end gap-2 max-sm:w-full max-sm:justify-start" data-plan-buttons="">
                            <button
                                type="button"
                                class="rounded-lg bg-primary px-3 py-2 text-xs font-medium text-primary-foreground shadow-xs hover:bg-primary/90 disabled:pointer-events-none disabled:opacity-50"
                                data-plan-action="implement"
                            >"Implement plan"</button>
                            <button
                                type="button"
                                class="rounded-lg border border-border bg-background px-3 py-2 text-xs font-medium shadow-xs hover:bg-foreground/5 disabled:pointer-events-none disabled:opacity-50"
                                data-plan-action="fresh"
                            >"Clear context and implement"</button>
                            <button
                                type="button"
                                class="rounded-lg px-3 py-2 text-xs text-muted-foreground hover:bg-foreground/5 hover:text-foreground disabled:pointer-events-none disabled:opacity-50"
                                data-plan-action="stay"
                            >"Stay in Plan mode"</button>
                        </div>
                    </div>
                </footer>
            }
        </article>
    }
}

async fn user_message(cx: &Cx, id: &str, item: &Value) -> Result {
    let content = item
        .get("content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    view! { cx =>
        <article
            id=(format!("item-{id}"))
            class="mb-7 flex justify-end"
            data-item-id=(id)
            data-client-id=(string(item, "clientId"))
        >
            <div class="message-content min-w-0 max-w-[82%] rounded-xl bg-foreground/[0.06] px-4 py-2.5 text-sm whitespace-pre-wrap">
                for input in content {
                    if string(&input, "type") == "text" {
                        (string(&input, "text"))
                    } else if matches!(string(&input, "type"), "image" | "localImage") {
                        <img
                            class="mt-2 max-h-80 max-w-full rounded-lg object-contain"
                            src=(input.get("url").or(input.get("path")).and_then(Value::as_str).unwrap_or_default())
                            alt="Attached image"
                        >
                    }
                }
            </div>
        </article>
    }
}

async fn agent_message(cx: &Cx, id: &str, item: &Value) -> Result {
    let markdown = markdown(string(item, "text"));
    view! { cx =>
        <article id=(format!("item-{id}")) class="prose mb-7 max-w-none text-sm leading-7" data-item-id=(id)>
            (Unescaped::new_unchecked(markdown))
        </article>
    }
}

async fn activity_card(cx: &Cx, id: &str, title: &str, output: String, item: &Value) -> Result {
    let status = string(item, "status");
    let active = matches!(status, "inProgress" | "running");
    let output = truncate_output(&output);
    view! { cx =>
        <article id=(format!("item-{id}")) class="mb-4" data-item-id=(id) data-activity-status=(status)>
            <details open=(active) class="activity-card group rounded-lg border border-border bg-foreground/[0.018]">
                <summary class="flex cursor-pointer list-none items-center gap-2 px-3 py-2 text-sm font-medium">
                    <span class="activity-caret text-muted-foreground group-open:rotate-90">"›"</span>
                    if active {
                        <span class="live-rail h-4 w-0.5 rounded-full bg-sky-500"></span>
                    }
                    <span class="truncate">(title)</span>
                    if !status.is_empty() {
                        badge(variant: BadgeVariant::Outline, attrs: attributes! { class="ml-auto" }, (status))
                    }
                </summary>
                if !output.is_empty() {
                    <pre class="max-h-96 overflow-auto border-t border-border p-3 font-mono text-xs whitespace-pre-wrap">(output)</pre>
                }
            </details>
        </article>
    }
}

pub(crate) async fn approval_fragment(cx: &Cx, request: &Value) -> Result {
    let id = request
        .get("id")
        .map(|id| {
            id.as_str()
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| id.to_string())
        })
        .unwrap_or_default();
    let method = string(request, "method");
    let params = request.get("params").unwrap_or(&Value::Null);
    let title = match method {
        "item/commandExecution/requestApproval" => "Run this command?",
        "item/fileChange/requestApproval" => "Apply these changes?",
        "item/tool/requestUserInput" => "Codex has a question",
        "item/permissions/requestApproval" => "Grant additional access?",
        "item/mcpToolCall/requestApproval" => "Allow this MCP tool?",
        _ => "Codex needs approval",
    };
    let detail = params
        .get("command")
        .or_else(|| params.get("reason"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| pretty(Some(params)));
    view! { cx =>
        <article
            id=(format!("approval-{id}"))
            class="mb-4"
            data-request-id=(id)
            data-method=(method)
            data-params=(params.to_string())
        >
            card(
                attrs: attributes! { class="gap-3 border-amber-400/60 bg-amber-500/[0.04] py-4" },
                card_header(
                    attrs: attributes! { class="px-4" },
                    card_title(attrs: attributes! { class="text-sm" }, (title))
                )
                card_content(
                    attrs: attributes! { class="px-4" },
                    if method == "item/tool/requestUserInput" {
                        <div class="grid gap-3">
                            for question in params.get("questions").and_then(Value::as_array).into_iter().flatten() {
                                <label class="grid gap-1.5 text-xs" data-question-id=(string(question, "id"))>
                                    <span class="font-medium">(string(question, "question"))</span>
                                    if let Some(options) = question.get("options").and_then(Value::as_array) {
                                        <select data-user-answer="" class="h-9 rounded-lg border border-border bg-background px-2">
                                            for option in options {
                                                <option value=(string(option, "label"))>(string(option, "label"))</option>
                                            }
                                        </select>
                                    } else {
                                        <input data-user-answer="" class="h-9 rounded-lg border border-border bg-background px-2" placeholder="Your answer">
                                    }
                                </label>
                            }
                        </div>
                    } else {
                        <pre class="max-h-48 overflow-auto font-mono text-xs whitespace-pre-wrap">(detail)</pre>
                    }
                )
                card_footer(
                    attrs: attributes! { class="px-4" },
                    button(
                        size: ButtonSize::Sm,
                        attrs: attributes! { type="button" data-decision="accept" },
                        "Allow"
                    )
                    if method != "item/tool/requestUserInput" {
                        button(
                            variant: ButtonVariant::Outline,
                            size: ButtonSize::Sm,
                            attrs: attributes! { type="button" data-decision="acceptForSession" },
                            "Always allow"
                        )
                    }
                    button(
                        variant: ButtonVariant::Ghost,
                        size: ButtonSize::Sm,
                        attrs: attributes! { type="button" data-decision="decline" },
                        "Deny"
                    )
                )
            )
        </article>
    }
}

fn actionable_plan_id(thread: &Value) -> Option<&str> {
    if string(thread, "id").is_empty() {
        return None;
    }
    let latest_turn = thread.get("turns")?.as_array()?.last()?;
    if string(latest_turn, "status") != "completed" {
        return None;
    }
    latest_turn
        .get("items")?
        .as_array()?
        .iter()
        .rev()
        .find(|item| string(item, "type") == "plan")
        .map(|item| string(item, "id"))
}

pub(crate) fn thread_title(thread: &Value) -> &str {
    ["name", "preview"]
        .into_iter()
        .find_map(|key| {
            thread
                .get(key)
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
        })
        .unwrap_or("New task")
}

fn thread_is_active(thread: &Value) -> bool {
    thread
        .pointer("/thread/status/type")
        .or_else(|| thread.pointer("/status/type"))
        .or_else(|| thread.get("status"))
        .and_then(Value::as_str)
        == Some("active")
}

fn model_label(model: &Value) -> &str {
    ["displayName", "model", "id"]
        .into_iter()
        .find_map(|key| {
            model
                .get(key)
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
        })
        .unwrap_or("Model")
}

fn string<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn bool_value(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or_default()
}

fn model_matches(model: &Value, active_model: &str) -> bool {
    !active_model.is_empty()
        && [string(model, "id"), string(model, "model")].contains(&active_model)
}

fn model_efforts(model: &Value) -> String {
    model
        .get("supportedReasoningEfforts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|effort| string(effort, "reasoningEffort"))
        .collect::<Vec<_>>()
        .join(",")
}

fn mode_effort(mode: &Value) -> &str {
    string(mode, "reasoning_effort")
}

fn effort_label(effort: &str) -> String {
    if effort == "xhigh" {
        return "XHigh".to_string();
    }
    let mut characters = effort.chars();
    characters
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + characters.as_str())
        .unwrap_or_default()
}

fn pretty(value: Option<&Value>) -> String {
    value
        .and_then(|value| serde_json::to_string_pretty(value).ok())
        .unwrap_or_default()
}

fn truncate_output(output: &str) -> String {
    const MAX_BYTES: usize = 256 * 1024;
    const MAX_LINES: usize = 2000;
    let line_start = output
        .match_indices('\n')
        .rev()
        .nth(MAX_LINES.saturating_sub(1))
        .map(|(index, _)| index + 1)
        .unwrap_or_default();
    let byte_start = output.len().saturating_sub(MAX_BYTES);
    let mut start = line_start.max(byte_start);
    while !output.is_char_boundary(start) {
        start += 1;
    }
    if start == 0 {
        output.to_string()
    } else {
        format!("… output truncated …\n{}", &output[start..])
    }
}

#[cfg(test)]
#[path = "views_tests.rs"]
mod tests;
