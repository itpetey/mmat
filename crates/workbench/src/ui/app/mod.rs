use dioxus::prelude::*;
use dioxus_icons::lucide::{ChevronRight, Circle, Plus};

use crate::{
    api::projects::{ProjectNavItem, create_project, list_projects},
    ui::{
        header::Header,
        vendor::{
            avatar::{Avatar, AvatarImageSize},
            button::{Button, ButtonVariant},
            collapsible::{Collapsible, CollapsibleContent, CollapsibleTrigger},
            dialog::{Dialog, DialogDescription, DialogTitle},
            dropdown_menu::{
                DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger,
            },
            separator::Separator,
            sidebar::{
                Sidebar, SidebarCollapsible, SidebarContent, SidebarFooter, SidebarGroup,
                SidebarGroupContent, SidebarGroupLabel, SidebarHeader, SidebarInset, SidebarMenu,
                SidebarMenuAction, SidebarMenuButton, SidebarMenuButtonSize, SidebarMenuItem,
                SidebarMenuSkeleton, SidebarMenuSub, SidebarMenuSubButton, SidebarMenuSubItem,
                SidebarProvider, SidebarRail, SidebarSide, SidebarTrigger, SidebarVariant,
            },
        },
    },
};

const DX_COMPONENT_CSS: Asset = asset!("/assets/dx-components-theme.css");
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
        // document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: MAIN_CSS }
        document::Link { rel: "stylesheet", href: TAILWIND_CSS }
        document::Link { rel: "stylesheet", href: DX_COMPONENT_CSS }
        SidebarProvider {
                        Sidebar { side: SidebarSide::Left, variant: SidebarVariant::Sidebar, collapsible: SidebarCollapsible::Offcanvas,
                            SidebarHeader {
                                ProjectSwitcher { projects }
                            }
                            SidebarContent {
                                NavMain { items: NAV_MAIN }
                                NavProjects { projects }
                            }
                            SidebarFooter { NavUser {} }
                            SidebarRail {}
                        }
                        SidebarInset {
                            header { style: "display:flex; align-items:center; justify-content:space-between; height:3.5rem; flex-shrink:0; padding:0 1rem; border-bottom:1px solid var(--dx-sidebar-border); background:var(--primary-color-1);",
                                div { style: "display: flex; align-items: center; gap: 0.75rem;",
                                    SidebarTrigger {}
                                    Separator { height: "1rem", horizontal: false }
                                    span { "Sidebar Setting" }
                                }
                            }
                            div { style: "display:flex; flex:1; flex-direction:column; gap:1.5rem; padding:1.5rem; min-height:0; overflow-y:auto; overflow-x:hidden;",
                                span { "blah" }
                            }
                        }




            Header {}
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
fn DemoSettingControls(
    side: Signal<SidebarSide>,
    collapsible: Signal<SidebarCollapsible>,
) -> Element {
    rsx! {
        div { style: "display: flex; flex-direction: column; gap: 0.75rem; padding: 0.75rem; border: 1px solid var(--dx-sidebar-border); border-radius: 0.75rem; background: var(--primary-color-2);",
            div { style: "display: flex; align-items: center; justify-content: space-between; gap: 0.75rem; flex-wrap: wrap;",
                span { style: "font-size: 0.75rem; font-weight: 600; color: var(--secondary-color-4);",
                    "Side"
                }
                div { style: "display: inline-flex; gap: 0.5rem;",
                    Button {
                        variant: if side() == SidebarSide::Left { ButtonVariant::Primary } else { ButtonVariant::Outline },
                        onclick: move |_| side.set(SidebarSide::Left),
                        style: "padding: 0.4rem 0.6rem; font-size: 0.75rem;",
                        "Left"
                    }
                    Button {
                        variant: if side() == SidebarSide::Right { ButtonVariant::Primary } else { ButtonVariant::Outline },
                        onclick: move |_| side.set(SidebarSide::Right),
                        style: "padding: 0.4rem 0.6rem; font-size: 0.75rem;",
                        "Right"
                    }
                }
            }
            div { style: "display: flex; align-items: center; justify-content: space-between; gap: 0.75rem; flex-wrap: wrap;",
                span { style: "font-size: 0.75rem; font-weight: 600; color: var(--secondary-color-4);",
                    "Collapse"
                }
                div { style: "display: inline-flex; gap: 0.5rem; flex-wrap: wrap;",
                    Button {
                        variant: if collapsible() == SidebarCollapsible::Offcanvas { ButtonVariant::Primary } else { ButtonVariant::Outline },
                        onclick: move |_| collapsible.set(SidebarCollapsible::Offcanvas),
                        style: "padding: 0.4rem 0.6rem; font-size: 0.75rem;",
                        "Offcanvas"
                    }
                    Button {
                        variant: if collapsible() == SidebarCollapsible::Icon { ButtonVariant::Primary } else { ButtonVariant::Outline },
                        onclick: move |_| collapsible.set(SidebarCollapsible::Icon),
                        style: "padding: 0.4rem 0.6rem; font-size: 0.75rem;",
                        "Icon"
                    }
                    Button {
                        variant: if collapsible() == SidebarCollapsible::None { ButtonVariant::Primary } else { ButtonVariant::Outline },
                        onclick: move |_| collapsible.set(SidebarCollapsible::None),
                        style: "padding: 0.4rem 0.6rem; font-size: 0.75rem;",
                        "None"
                    }
                }
            }
        }
    }
}

#[component]
fn NavMain(items: &'static [NavMainItem]) -> Element {
    rsx! {
        SidebarGroup {
            SidebarGroupLabel { "Platform" }
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

#[component]
fn NavProjects(projects: Resource<ServerFnResult<Vec<ProjectNavItem>>>) -> Element {
    rsx! {
        SidebarGroup { class: AppStyles::dx_sidebar_hide_on_collapse,
            SidebarGroupLabel { "Projects" }
            SidebarGroupContent {
                SidebarMenu {
                    match &*projects.read_unchecked() {
                        Some(Ok(items)) if !items.is_empty() => rsx! {
                            for project in items.iter() {
                                ProjectNavItemLink { key: "{project.id}", project: project.clone() }
                            }
                        },
                        Some(Ok(_)) => rsx! {
                            SidebarMenuItem {
                                SidebarMenuButton { style: "opacity:0.7;", "No projects yet" }
                            }
                        },
                        Some(Err(error)) => rsx! {
                            SidebarMenuItem {
                                SidebarMenuButton { "Unable to load projects: {error}" }
                            }
                        },
                        None => rsx! {
                            SidebarMenuItem { SidebarMenuSkeleton { show_icon: true } }
                            SidebarMenuItem { SidebarMenuSkeleton { show_icon: true } }
                            SidebarMenuItem { SidebarMenuSkeleton { show_icon: true } }
                        },
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
fn ProjectNavItemLink(project: ProjectNavItem) -> Element {
    let href = format!("/projects/{}", project.id);
    let label = project.label.clone();
    let title = project.path.clone();

    rsx! {
        SidebarMenuItem {
            SidebarMenuButton {
                as: move |attributes: Vec<Attribute>| {
                    let href = href.clone();
                    let label = label.clone();
                    let title = title.clone();

                    rsx! {
                        a { href, title, ..attributes,
                            DemoIcon {}
                            span { "{label}" }
                        }
                    }
                },
            }
            DropdownMenu { class: Styles::dx_sidebar_dropdown_menu,
                DropdownMenuTrigger { class: Styles::dx_sidebar_dropdown_menu_trigger,
                    as: move |attributes: Vec<Attribute>| rsx! {
                        SidebarMenuAction { show_on_hover: true, attributes,
                            DemoIcon {}
                            span { class: Styles::dx_sr_only, "More" }
                        }
                    },
                }
                DropdownMenuContent { class: Styles::dx_sidebar_dropdown_menu_content,
                    DropdownMenuItem {
                        index: 0usize,
                        value: "view".to_string(),
                        on_select: move |_: String| {},
                        DemoIcon {}
                        span { "View Project" }
                    }
                    Separator { class: Styles::dx_sidebar_dropdown_separator, decorative: true }
                    DropdownMenuItem {
                        index: 1usize,
                        value: "delete".to_string(),
                        on_select: move |_: String| {},
                        DemoIcon {}
                        span { "Delete Project" }
                    }
                }
            }
        }
    }
}

#[component]
fn ProjectSwitcher(mut projects: Resource<ServerFnResult<Vec<ProjectNavItem>>>) -> Element {
    let mut active_project = use_signal(|| 0usize);
    let mut dialog_open = use_signal(|| false);
    let mut project_label = use_signal(String::new);
    let mut project_path = use_signal(String::new);
    let mut create_error = use_signal(|| None::<String>);
    let mut is_creating = use_signal(|| false);

    rsx! {
        SidebarMenu {
            SidebarMenuItem {
                DropdownMenu { class: Styles::dx_sidebar_dropdown_menu,
                    DropdownMenuTrigger { class: Styles::dx_sidebar_dropdown_menu_trigger,
                        as: move |attributes: Vec<Attribute>| rsx! {
                            SidebarMenuButton { class: AppStyles::dx_sidebar_menu_disclosure_button, size: SidebarMenuButtonSize::Lg, attributes,
                                div { style: "display:flex; flex-shrink:0; align-items:center; justify-content:center; width:2rem; height:2rem; aspect-ratio:1; border-radius:0.5rem; background:var(--dx-sidebar-accent); color:var(--dx-sidebar-accent-foreground);",
                                    DemoIcon {}
                                }
                                match &*projects.read_unchecked() {
                                    Some(Ok(items)) if !items.is_empty() => {
                                        let project = &items[active_project().min(items.len() - 1)];
                                        rsx! {
                                            div { class: AppStyles::dx_sidebar_info_block,
                                                span { class: AppStyles::dx_sidebar_info_title, {project.label.clone()} }
                                                span { class: AppStyles::dx_sidebar_info_subtitle, {project.path.clone()} }
                                            }
                                        }
                                    }
                                    Some(Ok(_)) => rsx! {
                                        div { class: AppStyles::dx_sidebar_info_block,
                                            span { class: AppStyles::dx_sidebar_info_title, "No projects" }
                                            span { class: AppStyles::dx_sidebar_info_subtitle, "Create a project" }
                                        }
                                    },
                                    Some(Err(_)) => rsx! {
                                        div { class: AppStyles::dx_sidebar_info_block,
                                            span { class: AppStyles::dx_sidebar_info_title, "Projects unavailable" }
                                            span { class: AppStyles::dx_sidebar_info_subtitle, "Check server logs" }
                                        }
                                    },
                                    None => rsx! {
                                        div { class: AppStyles::dx_sidebar_info_block,
                                            span { class: AppStyles::dx_sidebar_info_title, "Loading projects" }
                                            span { class: AppStyles::dx_sidebar_info_subtitle, "Please wait" }
                                        }
                                    },
                                }
                                ChevronIcon {}
                            }
                        },
                    }
                    DropdownMenuContent { class: Styles::dx_sidebar_dropdown_menu_content,
                        div { style: "padding:0.5rem; font-size:0.75rem; opacity:0.7;",
                            "Projects"
                        }
                        match &*projects.read_unchecked() {
                            Some(Ok(items)) if !items.is_empty() => rsx! {
                                for (idx , project) in items.iter().enumerate() {
                                    DropdownMenuItem {
                                        index: idx,
                                        value: idx,
                                        on_select: move |v: usize| active_project.set(v),
                                        DemoIcon {}
                                        {project.label.clone()}
                                        span { style: "margin-left:auto; max-width:8rem; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; font-size:0.75rem; opacity:0.7;",
                                            {project.path.clone()}
                                        }
                                    }
                                }
                            },
                            Some(Ok(_)) => rsx! {
                                div { style: "padding:0.5rem; font-size:0.875rem; opacity:0.7;", "No projects found" }
                            },
                            Some(Err(error)) => rsx! {
                                div { style: "padding:0.5rem; font-size:0.875rem; color:#dc2626;", "Unable to load projects: {error}" }
                            },
                            None => rsx! {
                                div { style: "padding:0.5rem;", SidebarMenuSkeleton {} }
                            },
                        }
                        Separator { class: Styles::dx_sidebar_dropdown_separator, decorative: true }
                        DropdownMenuItem {
                            index: 999usize,
                            value: 999usize,
                            on_select: move |_: usize| {
                                create_error.set(None);
                                dialog_open.set(true);
                            },
                            Plus { size: "16px" }
                            div { style: "opacity:0.7; font-weight:500;", "Add project" }
                        }
                    }
                }
            }
        }
        Dialog {
            open: dialog_open(),
            on_open_change: move |open| dialog_open.set(open),
            DialogTitle { "Add Project" }
            DialogDescription { "Create a project entry in the memory database." }
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
                            Ok(_) => {
                                project_label.set(String::new());
                                project_path.set(String::new());
                                active_project.set(0);
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
                    "Project Name"
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
                    "Project Path"
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
