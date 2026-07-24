use topcoat::Result;
use topcoat::view::Attributes;
use topcoat::view::View;
use topcoat::view::class;
use topcoat::view::component;
use topcoat::view::view;

/// The classes for the native `<select>` inside the [`select`] component.
///
/// Sized to match the input control. The native dropdown arrow is suppressed
/// so the component can draw its own chevron, which keeps the control looking
/// the same across browsers; the extra right padding reserves the chevron's
/// space.
const SELECT: &str = "h-9 w-full appearance-none items-center rounded-lg border border-border \
    bg-background pr-8 pl-3 text-left text-sm shadow-xs transition-colors outline-none \
    focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 \
    focus-visible:ring-offset-background disabled:pointer-events-none";

/// The classes restyling the drop-down picker, for browsers that support
/// customizable selects (`appearance: base-select`, set on the `<select>` by
/// the component's wrapper).
///
/// The panel and its option rows take after the dropdown menu's content and
/// items: the same raised surface, the same ghost-tinted hover and focus
/// states, and the checked option marked by a checkmark on the row's right
/// edge: the [`CHECKMARK`] icon, masked over the theme's muted foreground
/// (see [`checkmark_style`]). The browser's own picker icon is hidden in
/// favor of the component's chevron. On browsers without support every rule
/// here is inert and the operating system's picker shows instead.
/// A select component: a themed native `<select>`.
///
/// Child nodes become the `<select>`'s content, typically `<option>` and
/// `<optgroup>` elements. The `attrs` (such as `name`, `disabled`, or event
/// handlers) are forwarded to the `<select>`; a `class` among them is appended
/// to the wrapping element's classes, so width utilities size the whole
/// control. Like the input, it fills its container by default.
///
/// On browsers with customizable select support the drop-down picker is
/// restyled to match the dropdown menu component, and the chevron flips while
/// it is open; other browsers keep the operating system's picker. The control
/// itself looks the same everywhere.
///
/// ```rust
/// view! {
///     select(
///         attrs: attributes! { name="region" },
///         <option>"eu-central-1"</option>
///         <option>"us-east-1"</option>
///     )
/// }
/// ```
#[component]
pub async fn select(#[default] mut attrs: Attributes, #[default] child: View) -> Result {
    // `appearance: base-select` opts into the customizable picker. It is set
    // from the wrapper because the descendant selector outranks the
    // `appearance-none` fallback in specificity, making the outcome
    // independent of stylesheet order; browsers without support drop the
    // invalid declaration and keep the fallback.
    view! {
        <span
            class=(class!(
                "relative block has-[:disabled]:opacity-50 \
                 [&>select]:[appearance:base-select]",
                attrs.remove("class"),
            ))
        >
            <select class=(class!(SELECT)) (attrs)>(child)</select>
            <span class="pointer-events-none absolute top-1/2 right-3 -translate-y-1/2 text-xs text-muted-foreground">"⌄"</span>
        </span>
    }
}
