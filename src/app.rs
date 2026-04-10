use codemirror::{Editor, EditorOptions};
use leptos::prelude::*;
use leptos::web_sys::HtmlTextAreaElement;
use wasm_bindgen::JsCast;

#[component]
pub fn App() -> impl IntoView {
    let textarea_ref = NodeRef::<leptos::html::Textarea>::new();

    textarea_ref.on_load(move |textarea| {
        let el: &HtmlTextAreaElement = &textarea;
        let options = EditorOptions::default().line_numbers(true);
        let _editor = Editor::from_text_area(el, &options);

        // CodeMirror sets min-height as an inline style on .CodeMirror-scroll,
        // which beats any stylesheet rule. Override it directly after init.
        let document = leptos::web_sys::window().unwrap().document().unwrap();
        if let Some(scroll) = document.query_selector(".CodeMirror-scroll").unwrap() {
            let style = scroll
                .unchecked_into::<leptos::web_sys::HtmlElement>()
                .style();
            style.set_property("min-height", "0").unwrap();
            style.set_property("height", "100%").unwrap();
        }
    });

    view! {
        <textarea node_ref=textarea_ref></textarea>
    }
}
