use pulldown_cmark::CowStr;
use pulldown_cmark::Event;
use pulldown_cmark::Options;
use pulldown_cmark::Parser;
use pulldown_cmark::Tag;
use pulldown_cmark::html;

pub(super) fn markdown(source: &str) -> String {
    let parser = Parser::new_ext(source, Options::all()).map(|event| match event {
        Event::Html(raw) | Event::InlineHtml(raw) => {
            Event::Text(CowStr::Boxed(raw.to_string().into_boxed_str()))
        }
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Link {
            link_type,
            dest_url: safe_destination(dest_url),
            title,
            id,
        }),
        Event::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Image {
            link_type,
            dest_url: safe_destination(dest_url),
            title,
            id,
        }),
        event => event,
    });
    let mut output = String::new();
    html::push_html(&mut output, parser);
    output.replace(
        "<a href=\"http",
        "<a target=\"_blank\" rel=\"noopener noreferrer\" href=\"http",
    )
}

fn safe_destination(destination: CowStr<'_>) -> CowStr<'_> {
    let normalized = destination.trim().to_ascii_lowercase();
    if normalized.starts_with("javascript:")
        || normalized.starts_with("data:")
        || normalized.starts_with("vbscript:")
    {
        CowStr::Borrowed("")
    } else {
        destination
    }
}
