use dioxus::prelude::*;

use crate::components::{
    Header, Projects,
    sidebar::{
        Sidebar, SidebarCollapsible, SidebarContent, SidebarHeader, SidebarProvider, SidebarRail,
        SidebarSide, SidebarVariant,
    },
};

// const FAVICON: Asset = asset!("/assets/favicon.ico");
// const HEADER_SVG: Asset = asset!("/assets/header.svg");
const MAIN_CSS: Asset = asset!("/assets/main.css");
const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

#[component]
pub fn App() -> Element {
    rsx! {
        // document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: MAIN_CSS }
        document::Link { rel: "stylesheet", href: TAILWIND_CSS }
        SidebarProvider {
            Sidebar { side: SidebarSide::Left, variant: SidebarVariant::Sidebar, collapsible: SidebarCollapsible::Offcanvas,
                SidebarHeader { "Projects" }
                SidebarContent { Projects {} }
                SidebarRail {}
            }
            Header {}
        }
    }
}
