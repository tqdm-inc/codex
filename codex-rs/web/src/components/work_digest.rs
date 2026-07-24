use serde_json::Value;
use topcoat::Result;
use topcoat::context::Cx;
use topcoat::view::view;

pub(crate) fn is_activity(item: &Value) -> bool {
    matches!(
        string(item, "type"),
        "reasoning"
            | "commandExecution"
            | "fileChange"
            | "mcpToolCall"
            | "dynamicToolCall"
            | "collabAgentToolCall"
            | "webSearch"
            | "imageView"
            | "imageGeneration"
            | "contextCompaction"
    )
}

pub(crate) fn is_visible_activity(item: &Value) -> bool {
    is_activity(item)
        && (string(item, "type") != "reasoning" || !reasoning_text(item).trim().is_empty())
}

pub(crate) async fn work_digest(
    cx: &Cx,
    turn_id: &str,
    turn_status: &str,
    items: &[Value],
) -> Result {
    let active = turn_status == "inProgress"
        || items
            .iter()
            .any(|item| matches!(string(item, "status"), "inProgress" | "running"));
    let failed = items
        .iter()
        .filter(|item| string(item, "status") == "failed")
        .count();
    let completed = items
        .iter()
        .filter(|item| string(item, "status") == "completed")
        .count();
    let open = active || failed > 0;
    let summary = digest_summary(items, active, completed, failed);
    view! { cx =>
        <article
            id=(format!("work-{turn_id}"))
            class="work-digest mb-5 min-w-0"
            data-work-digest=""
            data-turn-id=(turn_id)
            data-turn-status=(turn_status)
        >
            <details open=(open) class="group overflow-hidden rounded-xl border border-border bg-foreground/[0.012]">
                <summary class="flex min-h-10 cursor-pointer list-none items-center gap-2 px-3 text-xs text-muted-foreground">
                    <span class="activity-caret shrink-0 group-open:rotate-90">"›"</span>
                    if active {
                        <span data-work-live="" class="live-rail h-4 w-0.5 shrink-0 rounded-full bg-sky-500"></span>
                    }
                    <strong class="shrink-0 font-medium text-foreground">"Work"</strong>
                    <span data-work-summary="" class="min-w-0 flex-1 truncate">(summary)</span>
                    <span data-work-failure="" class="shrink-0 text-destructive" hidden=(failed == 0)>(format!("{failed} failed"))</span>
                </summary>
                <div data-work-items="" class="divide-y divide-border border-t border-border">
                    for item in items {
                        (activity_row(cx, item).await?)
                    }
                </div>
            </details>
        </article>
    }
}

pub(crate) async fn activity_row(cx: &Cx, item: &Value) -> Result {
    let id = string(item, "id");
    let status = string(item, "status");
    let active = matches!(status, "inProgress" | "running");
    let failed = status == "failed";
    let title = activity_title(item);
    let output = truncate_output(&activity_output(item));
    view! { cx =>
        <div
            id=(format!("item-{id}"))
            class="activity-row min-w-0"
            data-item-id=(id)
            data-activity-status=(status)
            data-activity-title=(title.as_str())
            data-activity-kind=(string(item, "type"))
        >
            <details open=(active || failed) class="group/sub">
                <summary class="flex min-h-9 cursor-pointer list-none items-center gap-2 px-3 py-1.5 text-xs">
                    <span class="activity-caret shrink-0 text-muted-foreground group-open/sub:rotate-90">"›"</span>
                    <span class=(if active {
                        "size-1.5 shrink-0 rounded-full bg-sky-500"
                    } else if failed {
                        "size-1.5 shrink-0 rounded-full bg-destructive"
                    } else {
                        "size-1.5 shrink-0 rounded-full bg-foreground/20"
                    })></span>
                    <span class="min-w-0 flex-1 truncate font-mono text-muted-foreground">(title)</span>
                    if failed {
                        <span class="shrink-0 text-destructive">"failed"</span>
                    } else if active {
                        <span class="shrink-0 text-sky-600 dark:text-sky-400">"running"</span>
                    }
                </summary>
                if !output.is_empty() {
                    <pre class="max-h-80 overflow-auto border-t border-border bg-background/35 px-4 py-3 font-mono text-xs whitespace-pre-wrap">(output)</pre>
                }
            </details>
        </div>
    }
}

fn digest_summary(items: &[Value], active: bool, completed: usize, failed: usize) -> String {
    if active
        && let Some(title) = items
            .iter()
            .rev()
            .find(|item| matches!(string(item, "status"), "inProgress" | "running"))
            .map(activity_title)
    {
        return title;
    }
    let actions = items.len();
    match (completed, failed) {
        (_, failed) if failed > 0 => format!("{actions} actions"),
        (completed, _) if completed > 0 => format!("{completed} actions completed"),
        _ => format!("{actions} actions"),
    }
}

fn activity_title(item: &Value) -> String {
    match string(item, "type") {
        "reasoning" => "Thinking".to_string(),
        "commandExecution" => format!("$ {}", string(item, "command")),
        "fileChange" => "File changes".to_string(),
        "mcpToolCall" => format!("{} · {}", string(item, "server"), string(item, "tool")),
        "dynamicToolCall" => string(item, "tool").to_string(),
        "collabAgentToolCall" => "Agent activity".to_string(),
        "webSearch" => "Web search".to_string(),
        "imageView" | "imageGeneration" => "Image".to_string(),
        "contextCompaction" => "Context compacted".to_string(),
        item_type => item_type.to_string(),
    }
}

fn activity_output(item: &Value) -> String {
    match string(item, "type") {
        "reasoning" => reasoning_text(item),
        "commandExecution" => string(item, "aggregatedOutput").to_string(),
        "fileChange" => item
            .get("output")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| pretty(item.get("changes"))),
        "mcpToolCall" => pretty(item.get("result").or(item.get("arguments"))),
        "webSearch" => string(item, "query").to_string(),
        "contextCompaction" => String::new(),
        _ => pretty(Some(item)),
    }
}

fn reasoning_text(item: &Value) -> String {
    ["summary", "content"]
        .into_iter()
        .filter_map(|key| item.get(key).and_then(Value::as_array))
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join("\n\n")
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

fn string<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

#[cfg(test)]
#[path = "work_digest_tests.rs"]
mod tests;
