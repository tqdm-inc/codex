use pretty_assertions::assert_eq;
use serde_json::json;
use topcoat::context::Cx;
use topcoat::view::Component;

use super::ComposerProps;
use super::DocumentData;
use super::PlanPresentation;
use super::SIDEBAR_LAYOUT;
use super::THREAD_LIST_LAYOUT;
use super::approval_fragment;
use super::bootstrap_document;
use super::composer;
use super::document;
use super::item_fragment;
use super::item_fragment_with_plan_presentation;
use super::markdown;

#[test]
fn markdown_escapes_raw_html() {
    assert_eq!(
        markdown("hello <script>alert(1)</script>"),
        "<p>hello &lt;script&gt;alert(1)&lt;/script&gt;</p>\n"
    );
}

#[test]
fn markdown_removes_unsafe_link_destinations() {
    assert_eq!(
        markdown("[bad](javascript:alert(1)) [good](https://example.com)"),
        "<p><a href=\"\">bad</a> <a target=\"_blank\" rel=\"noopener noreferrer\" href=\"https://example.com\">good</a></p>\n"
    );
}

#[tokio::test]
async fn renders_public_bootstrap_page() {
    let cx = Cx::default();
    let view = bootstrap_document(&cx).await.expect("render bootstrap");

    insta::assert_snapshot!(view.render(&cx));
}

#[test]
fn sidebar_layout_fits_viewport_and_scrolls_threads() {
    insta::assert_snapshot!(format!(
        "sidebar: {SIDEBAR_LAYOUT}\nthread list: {THREAD_LIST_LAYOUT}"
    ));
}

#[tokio::test]
async fn chat_feed_owns_the_main_scroll_area() {
    let cx = Cx::default();
    let view = document(
        &cx,
        DocumentData {
            base_path: "/i/instance",
            instance_id: "instance",
            base_seq: 0,
            threads: &[],
            active_thread: None,
            active_model: "",
            active_effort: "",
            active_permission_mode: "workspace",
            models: &[],
            collaboration_modes: &[],
            approvals: &[],
            workspace_cwd: "/workspace",
            mcp_servers: &[],
            accounts: &[],
            active_account: "Default",
            initial_next_cursor: None,
        },
    )
    .await
    .expect("render document");
    let rendered = view.render(&cx);

    insta::assert_snapshot!(format!(
        "main: {}\ntranscript: {}",
        class_for_id(&rendered, "chat-main"),
        class_for_id(&rendered, "transcript")
    ));
}

#[tokio::test]
async fn header_exposes_model_effort_and_mode_controls() {
    let cx = Cx::default();
    let models = json!([{
        "id": "gpt-test",
        "model": "gpt-test",
        "displayName": "GPT Test",
        "isDefault": true,
        "defaultReasoningEffort": "medium",
        "supportedReasoningEfforts": [
            { "reasoningEffort": "low", "description": "Fast" },
            { "reasoningEffort": "medium", "description": "Balanced" },
            { "reasoningEffort": "high", "description": "Deep" }
        ]
    }]);
    let collaboration_modes = json!([
        { "name": "Plan", "mode": "plan", "model": null, "reasoning_effort": "medium" },
        { "name": "Default", "mode": "default", "model": null, "reasoning_effort": null }
    ]);
    let view = document(
        &cx,
        DocumentData {
            base_path: "/i/instance",
            instance_id: "instance",
            base_seq: 0,
            threads: &[],
            active_thread: None,
            active_model: "gpt-test",
            active_effort: "high",
            active_permission_mode: "read-only",
            models: models.as_array().expect("models"),
            collaboration_modes: collaboration_modes.as_array().expect("collaboration modes"),
            approvals: &[],
            workspace_cwd: "/workspace",
            mcp_servers: &[],
            accounts: &[],
            active_account: "Default",
            initial_next_cursor: None,
        },
    )
    .await
    .expect("render document");
    let rendered = view.render(&cx);

    insta::assert_snapshot!(format!(
        "{}\n{}\n{}",
        select_summary(&rendered, "model-select"),
        select_summary(&rendered, "effort-select"),
        select_summary(&rendered, "mode-select")
    ));
}

#[tokio::test]
async fn header_constrains_long_thread_titles() {
    let cx = Cx::default();
    let active_thread = json!({
        "id": "thread-1",
        "name": "A very long conversation title that must not displace the header controls",
        "cwd": "/workspace/with/an/exceptionally/long/path/that/must/remain/constrained",
        "turns": []
    });
    let view = document(
        &cx,
        DocumentData {
            base_path: "/i/instance",
            instance_id: "instance",
            base_seq: 0,
            threads: &[],
            active_thread: Some(&active_thread),
            active_model: "",
            active_effort: "",
            active_permission_mode: "workspace",
            models: &[],
            collaboration_modes: &[],
            approvals: &[],
            workspace_cwd: "/workspace",
            mcp_servers: &[],
            accounts: &[],
            active_account: "Default",
            initial_next_cursor: None,
        },
    )
    .await
    .expect("render document");
    let rendered = view.render(&cx);
    let title_offset = rendered.find("id=\"thread-title\"").expect("thread title");
    let heading_start = rendered[..title_offset]
        .rfind("<div")
        .expect("heading start");
    let heading_end = rendered[title_offset..]
        .find("</div>")
        .map(|offset| title_offset + offset + "</div>".len())
        .expect("heading end");

    insta::assert_snapshot!(&rendered[heading_start..heading_end]);
}

#[tokio::test]
async fn composer_exposes_queue_and_run_states() {
    let cx = Cx::default();
    let view = composer::default()
        .render(
            &cx,
            ComposerProps {
                thread_id: "thread-1",
                permission_mode: "full-access",
            },
        )
        .await
        .expect("render composer");
    let rendered = view.render(&cx);
    let stop_id = "id=\"stop-button\"";
    assert_eq!(rendered.matches(stop_id).count(), 1);
    let stop_id_offset = rendered.find(stop_id).expect("stop button");
    let stop_tag_start = rendered[..stop_id_offset]
        .rfind('<')
        .expect("stop button opening tag");
    let stop_tag_end = rendered[stop_id_offset..]
        .find('>')
        .map(|offset| stop_id_offset + offset)
        .expect("stop button closing bracket");
    let stop_tag = &rendered[stop_tag_start..=stop_tag_end];
    assert!(stop_tag.starts_with("<button "));
    assert!(stop_tag.contains("type=\"button\""));
    assert!(stop_tag.contains("hidden"));
    assert!(stop_id_offset < rendered.find("id=\"send-button\"").expect("send button"));
    let ids = rendered
        .split(" id=\"")
        .skip(1)
        .filter_map(|rest| rest.split_once('"').map(|(id, _)| id))
        .collect::<Vec<_>>()
        .join("\n");
    let mut text = String::new();
    let mut inside_tag = false;
    for character in rendered.chars() {
        match character {
            '<' => inside_tag = true,
            '>' => inside_tag = false,
            _ if !inside_tag => text.push(character),
            _ => {}
        }
    }

    insta::assert_snapshot!(format!("ids:\n{ids}\n\ntext:\n{text}"));
}

#[tokio::test]
async fn renders_agent_message() {
    let cx = Cx::default();
    let item = json!({
        "id": "agent-1",
        "type": "agentMessage",
        "text": "# Result\n\nDone with **Topcoat** and `Rust`.\n\n- first\n- [x] shipped\n\n> Keep it maintainable.\n\n| Part | State |\n| --- | --- |\n| Web | Ready |\n\n[Open Codex](https://example.com) and ~~remove the fallback~~.\n\n```rust\nfn main() {}\n```"
    });
    let view = item_fragment(&cx, &item)
        .await
        .expect("render agent message");

    insta::assert_snapshot!(view.render(&cx));
}

#[tokio::test]
async fn renders_user_message_with_long_word_wrapping() {
    let cx = Cx::default();
    let item = json!({
        "id": "user-1",
        "type": "userMessage",
        "content": [{ "type": "text", "text": "averylongunbrokenvalue" }]
    });
    let view = item_fragment(&cx, &item)
        .await
        .expect("render user message");

    insta::assert_snapshot!(view.render(&cx));
}

#[tokio::test]
async fn renders_live_activity_expanded() {
    let cx = Cx::default();
    let item = json!({
        "id": "command-1",
        "type": "commandExecution",
        "command": "cargo check -p codex-web",
        "aggregatedOutput": "Checking codex-web",
        "status": "inProgress"
    });
    let view = item_fragment(&cx, &item).await.expect("render activity");

    insta::assert_snapshot!(view.render(&cx));
}

#[tokio::test]
async fn renders_actionable_plan_proposal() {
    let cx = Cx::default();
    let item = json!({
        "id": "plan-1",
        "type": "plan",
        "text": "# Ship the feature\n\n## Summary\n\nRender **Markdown**.\n\n- Implement it\n- Verify it"
    });
    let view = item_fragment_with_plan_presentation(&cx, &item, PlanPresentation::Actionable)
        .await
        .expect("render plan proposal");

    insta::assert_snapshot!(view.render(&cx));
}

#[tokio::test]
async fn renders_streaming_plan_without_actions() {
    let cx = Cx::default();
    let item = json!({
        "id": "plan-streaming",
        "type": "plan",
        "text": "# Draft plan\n\nStill writing…"
    });
    let view = item_fragment_with_plan_presentation(&cx, &item, PlanPresentation::Streaming)
        .await
        .expect("render streaming plan proposal");

    insta::assert_snapshot!(view.render(&cx));
}

#[tokio::test]
async fn only_latest_completed_turn_plan_is_actionable() {
    let cx = Cx::default();
    let thread = json!({
        "id": "thread-1",
        "turns": [
            {
                "id": "turn-1",
                "status": "completed",
                "items": [{ "id": "plan-old", "type": "plan", "text": "# Old" }]
            },
            {
                "id": "turn-2",
                "status": "completed",
                "items": [{ "id": "plan-new", "type": "plan", "text": "# New" }]
            }
        ]
    });
    let view = super::transcript_fragment(&cx, Some(&thread), &[])
        .await
        .expect("render transcript");
    let rendered = view.render(&cx);

    assert_eq!(rendered.matches("data-plan-actionable=\"true\"").count(), 1);
    assert!(
        rendered
            .split("id=\"item-plan-new\"")
            .nth(1)
            .is_some_and(|tail| tail.contains("data-plan-actionable=\"true\""))
    );
}

#[tokio::test]
async fn transcript_groups_activity_and_omits_empty_reasoning() {
    let cx = Cx::default();
    let thread = json!({
        "id": "thread-1",
        "turns": [{
            "id": "turn-1",
            "status": "completed",
            "items": [
                {
                    "id": "user-1",
                    "type": "userMessage",
                    "content": [{ "type": "text", "text": "Ship it" }]
                },
                {
                    "id": "reasoning-empty",
                    "type": "reasoning",
                    "summary": [],
                    "content": []
                },
                {
                    "id": "command-1",
                    "type": "commandExecution",
                    "command": "cargo test",
                    "status": "completed"
                },
                {
                    "id": "reasoning-1",
                    "type": "reasoning",
                    "summary": ["The tests passed."],
                    "content": [],
                    "status": "completed"
                },
                {
                    "id": "agent-1",
                    "type": "agentMessage",
                    "text": "Done."
                }
            ]
        }]
    });
    let view = super::transcript_fragment(&cx, Some(&thread), &[])
        .await
        .expect("render transcript");
    let rendered = view.render(&cx);

    assert_eq!(rendered.matches("data-work-digest").count(), 1);
    assert!(!rendered.contains("reasoning-empty"));
    insta::assert_snapshot!(rendered);
}

#[tokio::test]
async fn renders_user_input_request_as_a_form() {
    let cx = Cx::default();
    let request = json!({
        "id": "request-1",
        "method": "item/tool/requestUserInput",
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "questions": [{
                "id": "choice",
                "question": "Choose a mode",
                "options": [{ "label": "Default" }, { "label": "Plan" }]
            }]
        }
    });
    let view = approval_fragment(&cx, &request)
        .await
        .expect("render request");

    insta::assert_snapshot!(element_summary(&view.render(&cx)));
}

fn element_summary(rendered: &str) -> String {
    let ids = rendered
        .split(" id=\"")
        .skip(1)
        .filter_map(|rest| rest.split_once('"').map(|(id, _)| id))
        .collect::<Vec<_>>()
        .join(", ");
    let questions = rendered
        .split("data-question-id=\"")
        .skip(1)
        .filter_map(|rest| rest.split_once('"').map(|(id, _)| id))
        .collect::<Vec<_>>()
        .join(", ");
    let mut text = String::new();
    let mut inside_tag = false;
    for character in rendered.chars() {
        match character {
            '<' => inside_tag = true,
            '>' => inside_tag = false,
            _ if !inside_tag => text.push(character),
            _ => {}
        }
    }
    format!("ids: {ids}\nquestions: {questions}\ntext: {text}")
}

fn class_for_id<'a>(rendered: &'a str, id: &str) -> &'a str {
    let id_marker = format!("id=\"{id}\"");
    let id_offset = rendered.find(&id_marker).expect("element id");
    let tag_start = rendered[..id_offset].rfind('<').expect("tag start");
    let tag_end = rendered[id_offset..].find('>').expect("tag end") + id_offset;
    let tag = &rendered[tag_start..tag_end];
    let class_start = tag.find("class=\"").expect("class attribute") + "class=\"".len();
    let class_end = tag[class_start..].find('"').expect("class end") + class_start;
    &tag[class_start..class_end]
}

fn select_summary(rendered: &str, id: &str) -> String {
    let id_marker = format!("id=\"{id}\"");
    let id_offset = rendered.find(&id_marker).expect("select id");
    let select_start = rendered[..id_offset]
        .rfind("<select")
        .expect("select start");
    let select_end = rendered[id_offset..]
        .find("</select>")
        .map(|offset| id_offset + offset)
        .expect("select end");
    let select = &rendered[select_start..select_end];
    let options = select
        .split("<option")
        .skip(1)
        .map(|option| {
            let (attributes, remainder) = option.split_once('>').expect("option start");
            let label = remainder.split('<').next().expect("option label");
            let value = attribute_value(attributes, "value").unwrap_or_default();
            let selected = attributes.contains("selected");
            format!(
                "{value}={label}{}",
                if selected { " [selected]" } else { "" }
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{id}: {options}")
}

fn attribute_value<'a>(attributes: &'a str, name: &str) -> Option<&'a str> {
    let marker = format!("{name}=\"");
    let start = attributes.find(&marker)? + marker.len();
    let end = attributes[start..].find('"')? + start;
    Some(&attributes[start..end])
}
