use dioxus::prelude::*;

#[component]
pub fn Header() -> Element {
    rsx! {
        div {
            id: "header",
            // img { src: HEADER_SVG, id: "header" }
            span {
                "header"
            }
        }
    }
}
