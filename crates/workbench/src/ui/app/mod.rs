use dioxus::{document::Link, prelude::*};
use dioxus_icons::lucide::{ChevronRight, Circle, Plus};

use crate::{
    api::projects::{ProjectNavItem, create_project, list_projects},
    ui::{
        chat::ChatWorkbench,
        vendor::{
            avatar::{Avatar, AvatarImageSize},
            button::{Button, ButtonVariant},
            collapsible::{Collapsible, CollapsibleContent, CollapsibleTrigger},
            combobox::{Combobox, ComboboxEmpty, ComboboxOption},
            dialog::{Dialog, DialogDescription, DialogTitle},
            dropdown_menu::{
                DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger,
            },
            separator::Separator,
            sidebar::{
                Sidebar, SidebarCollapsible, SidebarContent, SidebarFooter, SidebarGroup,
                SidebarGroupContent, SidebarGroupLabel, SidebarHeader, SidebarInset, SidebarMenu,
                SidebarMenuButton, SidebarMenuButtonSize, SidebarMenuItem, SidebarMenuSub,
                SidebarMenuSubButton, SidebarMenuSubItem, SidebarProvider, SidebarRail,
                SidebarSide, SidebarTrigger, SidebarVariant,
            },
        },
    },
};

const DX_COMPONENT_CSS: Asset = asset!("/assets/dx-components-theme.css");
const ADD_PROJECT_VALUE: &str = "__add_project__";
const MAIN_CSS: Asset = asset!("/assets/main.css");
const NAV_MAIN: &[NavMainItem] = &[
    NavMainItem {
        title: "Playground",
        url: "#",
        is_active: true,
        items: &[
            SubItem {
                title: "History",
                url: "#",
            },
            SubItem {
                title: "Starred",
                url: "#",
            },
            SubItem {
                title: "Settings",
                url: "#",
            },
        ],
    },
    NavMainItem {
        title: "Models",
        url: "#",
        is_active: false,
        items: &[
            SubItem {
                title: "Genesis",
                url: "#",
            },
            SubItem {
                title: "Explorer",
                url: "#",
            },
            SubItem {
                title: "Quantum",
                url: "#",
            },
        ],
    },
    NavMainItem {
        title: "Documentation",
        url: "#",
        is_active: false,
        items: &[
            SubItem {
                title: "Introduction",
                url: "#",
            },
            SubItem {
                title: "Get Started",
                url: "#",
            },
            SubItem {
                title: "Tutorials",
                url: "#",
            },
            SubItem {
                title: "Changelog",
                url: "#",
            },
        ],
    },
    NavMainItem {
        title: "Settings",
        url: "#",
        is_active: false,
        items: &[
            SubItem {
                title: "General",
                url: "#",
            },
            SubItem {
                title: "Team",
                url: "#",
            },
            SubItem {
                title: "Billing",
                url: "#",
            },
            SubItem {
                title: "Limits",
                url: "#",
            },
        ],
    },
];
const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

#[css_module("/src/ui/app/style.css")]
struct AppStyles;

#[css_module("/src/ui/vendor/sidebar/style.css")]
struct Styles;

#[derive(Clone, PartialEq)]
struct SubItem {
    title: &'static str,
    url: &'static str,
}

#[derive(Clone, PartialEq)]
struct NavMainItem {
    title: &'static str,
    url: &'static str,
    is_active: bool,
    items: &'static [SubItem],
}

#[component]
pub fn App() -> Element {
    let projects = use_resource(|| async move { list_projects().await });

    rsx! {
        // Link { rel: "icon", href: FAVICON }
        Link { rel: "stylesheet", href: MAIN_CSS }
        Link { rel: "stylesheet", href: TAILWIND_CSS }
        Link { rel: "stylesheet", href: DX_COMPONENT_CSS }
        SidebarProvider {
            Sidebar { side: SidebarSide::Left, variant: SidebarVariant::Sidebar, collapsible: SidebarCollapsible::Offcanvas,
                SidebarHeader {
                    ProjectSwitcher { projects }
                }
                SidebarContent {
                    NavMain { items: NAV_MAIN }
                }
                SidebarFooter { NavUser {} }
                SidebarRail {}
            }
            SidebarInset {
                header { style: "display:flex; align-items:center; justify-content:space-between; height:3.5rem; flex-shrink:0; padding:0 1rem; border-bottom:1px solid var(--dx-sidebar-border); background:var(--primary-color-1);",
                    div { style: "display: flex; align-items: center; gap: 0.75rem;",
                        SidebarTrigger {}
                        Separator { height: "1rem", horizontal: false }
                        span { "Conversation" }
                    }
                }
                ChatWorkbench {}
            }
        }
    }
}

#[component]
fn ChevronIcon() -> Element {
    rsx! {
        ChevronRight {
            class: format!("{} {}", AppStyles::dx_sidebar_icon, AppStyles::dx_sidebar_chevron),
            size: "24px",
        }
    }
}

#[component]
fn DemoIcon() -> Element {
    rsx! {
        Circle {
            class: AppStyles::dx_sidebar_icon,
            size: "24px",
        }
    }
}

#[component]
fn NavMain(items: &'static [NavMainItem]) -> Element {
    rsx! {
        SidebarGroup {
            SidebarGroupLabel { "Platform" }
            SidebarGroupContent {
                SidebarMenu {
                    for item in items.iter() {
                        Collapsible {
                            default_open: item.is_active,
                            as: move |attributes: Vec<Attribute>| rsx! {
                                SidebarMenuItem { key: "{item.title}", attributes,
                                    CollapsibleTrigger { class: Styles::dx_sidebar_collapsible_trigger,
                                        as: move |attributes: Vec<Attribute>| rsx! {
                                            SidebarMenuButton {
                                                class: AppStyles::dx_sidebar_menu_disclosure_button,
                                                tooltip: rsx! {
                                                    {item.title}
                                                },
                                                attributes,
                                                DemoIcon {}
                                                span { {item.title} }
                                                ChevronIcon {}
                                            }
                                        },
                                    }
                                    CollapsibleContent {
                                        SidebarMenuSub {
                                            for sub_item in item.items {
                                                SidebarMenuSubItem { key: "{sub_item.title}",
                                                    SidebarMenuSubButton {
                                                        as: move |attributes: Vec<Attribute>| rsx! {
                                                            a { href: sub_item.url, ..attributes,
                                                                span { {sub_item.title} }
                                                            }
                                                        },
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            },
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn NavUser() -> Element {
    rsx! {
        SidebarMenu {
            SidebarMenuItem {
                DropdownMenu { class: Styles::dx_sidebar_dropdown_menu,
                    DropdownMenuTrigger { class: Styles::dx_sidebar_dropdown_menu_trigger,
                        as: move |attributes: Vec<Attribute>| rsx! {
                            SidebarMenuButton { class: AppStyles::dx_sidebar_menu_disclosure_button, size: SidebarMenuButtonSize::Lg, attributes,
                                Avatar {
                                    size: AvatarImageSize::Small,
                                    style: "border-radius:0.5rem;",
                                    src: asset!("/assets/ol_gil.jpg", ImageAssetOptions::new().with_avif()).to_string(),
                                    alt: "dioxus avatar",
                                    "DX"
                                }
                                div { class: AppStyles::dx_sidebar_info_block,
                                    span { class: AppStyles::dx_sidebar_info_title, "Dioxus" }
                                    span { class: AppStyles::dx_sidebar_info_subtitle, "m@example.com" }
                                }
                                ChevronIcon {}
                            }
                        },
                    }
                    DropdownMenuContent { class: Styles::dx_sidebar_dropdown_menu_content,
                        div { style: "display:flex; align-items:center; gap:0.5rem; padding:0.375rem 0.25rem; text-align:left; font-size:0.875rem;",
                            Avatar {
                                size: AvatarImageSize::Small,
                                style: "border-radius:0.5rem;",
                                src: asset!("/assets/ol_gil.jpg", ImageAssetOptions::new().with_avif()).to_string(),
                                alt: "dioxus avatar",
                                "DX"
                            }
                            div { class: AppStyles::dx_sidebar_info_block,
                                span { class: AppStyles::dx_sidebar_info_title, "Dioxus" }
                                span { class: AppStyles::dx_sidebar_info_subtitle, "m@example.com" }
                            }
                        }
                        Separator { class: Styles::dx_sidebar_dropdown_separator, decorative: true }
                        DropdownMenuItem {
                            index: 0usize,
                            value: "upgrade".to_string(),
                            on_select: move |_: String| {},
                            DemoIcon {}
                            "Upgrade to Pro"
                        }
                        Separator { class: Styles::dx_sidebar_dropdown_separator, decorative: true }
                        DropdownMenuItem {
                            index: 1usize,
                            value: "account".to_string(),
                            on_select: move |_: String| {},
                            DemoIcon {}
                            "Account"
                        }
                        DropdownMenuItem {
                            index: 2usize,
                            value: "billing".to_string(),
                            on_select: move |_: String| {},
                            DemoIcon {}
                            "Billing"
                        }
                        DropdownMenuItem {
                            index: 3usize,
                            value: "notifications".to_string(),
                            on_select: move |_: String| {},
                            DemoIcon {}
                            "Notifications"
                        }
                        Separator { class: Styles::dx_sidebar_dropdown_separator, decorative: true }
                        DropdownMenuItem {
                            index: 4usize,
                            value: "logout".to_string(),
                            on_select: move |_: String| {},
                            DemoIcon {}
                            "Log out"
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn ProjectSwitcher(mut projects: Resource<ServerFnResult<Vec<ProjectNavItem>>>) -> Element {
    let mut active_project = use_signal(|| None::<String>);
    let mut dialog_open = use_signal(|| false);
    let mut combobox_open = use_signal(|| false);
    let mut project_label = use_signal(String::new);
    let mut project_path = use_signal(String::new);
    let mut create_error = use_signal(|| None::<String>);
    let mut is_creating = use_signal(|| false);

    use_effect(move || {
        if let Some(Ok(items)) = &*projects.read() {
            let current = active_project();

            if items.is_empty() {
                if current.is_some() {
                    active_project.set(None);
                }

                return;
            }

            let needs_selection = current
                .as_ref()
                .is_none_or(|id| !items.iter().any(|project| project.id == *id));

            if needs_selection {
                active_project.set(Some(items[0].id.clone()));
            }
        }
    });

    rsx! {
        SidebarMenu {
            SidebarMenuItem {
                Combobox::<String> {
                    class: AppStyles::dx_project_combobox,
                    value: Some(active_project.into()),
                    on_open_change: move |open| combobox_open.set(open),
                    on_value_change: move |value: Option<String>| match value.as_deref() {
                        Some(ADD_PROJECT_VALUE) => {
                            create_error.set(None);
                            dialog_open.set(true);
                        }
                        Some(id) => active_project.set(Some(id.to_string())),
                        None => active_project.set(None),
                    },
                    placeholder: "Select project...",
                    aria_label: "Project",
                    list_aria_label: "Projects",

                    match &*projects.read_unchecked() {
                        Some(Ok(items)) if !items.is_empty() => rsx! {
                            for (idx , project) in items.iter().enumerate() {
                                ComboboxOption::<String> {
                                    index: idx,
                                    value: project.id.clone(),
                                    text_value: project.label.clone(),
                                    DemoIcon {}
                                    div { class: AppStyles::dx_project_combobox_option_text,
                                        span { class: AppStyles::dx_project_combobox_option_title, {project.label.clone()} }
                                        span { class: AppStyles::dx_project_combobox_option_path, {project.path.clone()} }
                                    }
                                }
                            }
                        },
                        Some(Ok(_)) if combobox_open() => rsx! {
                            div { class: AppStyles::dx_project_combobox_message, "No projects found" }
                        },
                        Some(Ok(_)) | None => rsx! {},
                        Some(Err(error)) => rsx! {
                            div { class: AppStyles::dx_project_combobox_error, "Unable to load projects: {error}" }
                        },
                    }

                    ComboboxEmpty { "No matching projects." }
                    ComboboxOption::<String> {
                        index: 999usize,
                        value: ADD_PROJECT_VALUE.to_string(),
                        text_value: "Add project".to_string(),
                        Plus { size: "16px" }
                        div { class: AppStyles::dx_project_combobox_option_text,
                            span { class: AppStyles::dx_project_combobox_option_title, "Add project" }
                        }
                    }
                }
            }
        }
        Dialog {
            open: dialog_open(),
            on_open_change: move |open| dialog_open.set(open),
            DialogTitle { "Add Project" }
            DialogDescription { "Create a new project at the given path." }
            form {
                class: "mt-4 flex flex-col gap-4",
                onsubmit: move |event| {
                    event.prevent_default();

                    let label = project_label().trim().to_string();
                    let path = project_path().trim().to_string();

                    if label.is_empty() || path.is_empty() {
                        create_error.set(Some("Project name and path are required.".to_string()));
                        return;
                    }

                    is_creating.set(true);
                    create_error.set(None);

                    spawn(async move {
                        match create_project(label, path).await {
                            Ok(project) => {
                                project_label.set(String::new());
                                project_path.set(String::new());
                                active_project.set(Some(project.id));
                                dialog_open.set(false);
                                projects.restart();
                            }
                            Err(error) => {
                                create_error.set(Some(error.to_string()));
                            }
                        }

                        is_creating.set(false);
                    });
                },
                label {
                    class: "flex flex-col gap-1 text-sm",
                    r#for: "project-label",
                    "Name"
                    input {
                        id: "project-label",
                        class: "rounded-md border px-3 py-2 text-sm",
                        value: "{project_label}",
                        disabled: is_creating(),
                        oninput: move |event| project_label.set(event.value()),
                    }
                }
                label {
                    class: "flex flex-col gap-1 text-sm",
                    r#for: "project-path",
                    "Path"
                    input {
                        id: "project-path",
                        class: "rounded-md border px-3 py-2 text-sm",
                        value: "{project_path}",
                        disabled: is_creating(),
                        oninput: move |event| project_path.set(event.value()),
                    }
                }
                if let Some(error) = create_error() {
                    p { class: "text-sm text-red-600", "{error}" }
                }
                div { class: "flex justify-end gap-2",
                    Button {
                        variant: ButtonVariant::Secondary,
                        r#type: "button",
                        disabled: is_creating(),
                        onclick: move |_| dialog_open.set(false),
                        "Cancel"
                    }
                    Button {
                        r#type: "submit",
                        disabled: is_creating(),
                        if is_creating() {
                            "Creating..."
                        } else {
                            "Create Project"
                        }
                    }
                }
            }
        }
    }
}
