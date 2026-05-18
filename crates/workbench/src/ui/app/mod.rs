use dioxus::{document::Link, prelude::*};
use dioxus_icons::lucide::{ChevronRight, Circle, Plus};
use dioxus_primitives::dioxus_attributes::attributes;

use crate::{
    api::chat::{
        SYSTEM_LANE_ID, WorkbenchLane, archive_lane as archive_lane_api,
        create_lane as create_lane_api, load_lanes,
    },
    api::projects::{ProjectNavItem, create_project, list_projects},
    ui::{
        chat::ChatWorkbench,
        vendor::{
            avatar::{Avatar, AvatarImageSize},
            button::{Button, ButtonVariant},
            combobox::{Combobox, ComboboxEmpty, ComboboxOption},
            dialog::{Dialog, DialogDescription, DialogTitle},
            dropdown_menu::{
                DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger,
            },
            separator::Separator,
            sidebar::{
                Sidebar, SidebarCollapsible, SidebarContent, SidebarFooter, SidebarGroup,
                SidebarGroupContent, SidebarGroupLabel, SidebarHeader, SidebarInset, SidebarMenu,
                SidebarMenuAction, SidebarMenuButton, SidebarMenuButtonSize, SidebarMenuItem,
                SidebarProvider, SidebarRail, SidebarSide, SidebarTrigger, SidebarVariant,
            },
            tabs::{TabContent, TabList, TabTrigger, Tabs},
        },
    },
};

const ADD_PROJECT_VALUE: &str = "__add_project__";
const DX_COMPONENT_CSS: Asset = asset!("/assets/dx-components-theme.css");
const MAIN_CSS: Asset = asset!("/assets/main.css");
const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

#[css_module("/src/ui/app/style.css")]
struct AppStyles;

#[css_module("/src/ui/vendor/sidebar/style.css")]
struct Styles;

#[component]
pub fn App() -> Element {
    let projects = use_resource(|| async move { list_projects().await });
    let selected_lane_id = use_signal(|| None::<String>);
    let selected_lane_status = use_signal(|| None::<String>);
    let selected_project_id = use_signal(|| None::<String>);
    let lanes_revision = use_signal(|| 0u64);

    rsx! {
        // Link { rel: "icon", href: FAVICON }
        Link { rel: "stylesheet", href: MAIN_CSS }
        Link { rel: "stylesheet", href: TAILWIND_CSS }
        Link { rel: "stylesheet", href: DX_COMPONENT_CSS }
        SidebarProvider {
            Sidebar { side: SidebarSide::Left, variant: SidebarVariant::Sidebar, collapsible: SidebarCollapsible::Offcanvas,
                SidebarHeader {
                    ProjectSwitcher { projects, selected_project_id, selected_lane_id, selected_lane_status }
                }
                SidebarContent {
                    LaneNavigation { selected_project_id, selected_lane_id, selected_lane_status, lanes_revision }
                }
                SidebarFooter { NavUser {} }
                SidebarRail {}
            }
            SidebarInset {
                Tabs {
                    class: AppStyles::app_tabs.to_string(),
                    default_value: "chat",
                    header { style: "display:flex; align-items:center; justify-content:space-between; height:3.5rem; flex-shrink:0; padding:0 1rem; border-bottom:1px solid var(--dx-sidebar-border); background:var(--primary-color-1);",
                        div { style: "display: flex; align-items: center; gap: 0.75rem;",
                            SidebarTrigger {}
                            Separator { height: "1rem", horizontal: false }
                            TabList {
                                TabTrigger { index: 0usize, value: "chat", "Chat" }
                                TabTrigger { index: 1usize, value: "graph", "Graph" }
                            }
                        }
                    }
                    TabContent {
                        index: 0usize,
                        value: "chat",
                        ChatWorkbench { selected_project_id, selected_lane_id, selected_lane_status, lanes_revision }
                    }
                    TabContent {
                        index: 1usize,
                        value: "graph",
                        span { "Hello Graph!" }
                    }
                }
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
fn LaneNavigation(
    selected_project_id: Signal<Option<String>>,
    mut selected_lane_id: Signal<Option<String>>,
    mut selected_lane_status: Signal<Option<String>>,
    lanes_revision: Signal<u64>,
) -> Element {
    let mut lanes = use_resource(move || async move {
        let _revision = lanes_revision();
        match selected_project_id() {
            Some(project_id) => load_lanes(project_id).await,
            None => Ok(crate::api::chat::LaneProjection {
                active: Vec::new(),
                archived: Vec::new(),
                system: WorkbenchLane {
                    id: SYSTEM_LANE_ID.to_string(),
                    title: "System".to_string(),
                    status: "system".to_string(),
                    system: true,
                },
            }),
        }
    });
    let mut title = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);

    use_effect(move || {
        if let Some(Ok(projection)) = &*lanes.read() {
            if let Some(selected_lane) = selected_lane_id() {
                let selected_status = projection
                    .active
                    .iter()
                    .chain(projection.archived.iter())
                    .chain(std::iter::once(&projection.system))
                    .find(|lane| lane.id == selected_lane)
                    .map(|lane| lane.status.clone());
                selected_lane_status.set(selected_status);
            } else if let Some(first) = projection.active.first() {
                selected_lane_id.set(Some(first.id.clone()));
                selected_lane_status.set(Some(first.status.clone()));
            }
        }
    });

    rsx! {
        SidebarGroup {
            SidebarGroupLabel { "Lanes" }
            SidebarGroupContent {
                SidebarMenu {
                    match &*lanes.read_unchecked() {
                        Some(Ok(projection)) => rsx! {
                            for lane in projection.active.iter() {
                                SidebarMenuItem {
                                    key: "{lane.id}",
                                    LaneButton {
                                        lane: lane.clone(),
                                        selected: selected_lane_id().as_deref() == Some(lane.id.as_str()),
                                        selected_lane_id,
                                        selected_lane_status,
                                    }
                                    ArchiveLaneButton {
                                        lane: lane.clone(),
                                        selected_project_id,
                                        selected_lane_id,
                                        selected_lane_status,
                                        lanes_revision,
                                    }
                                }
                            }
                            SidebarMenuItem {
                                LaneButton {
                                    lane: projection.system.clone(),
                                    selected: selected_lane_id().as_deref() == Some(SYSTEM_LANE_ID),
                                    selected_lane_id,
                                    selected_lane_status,
                                }
                            }
                        },
                        Some(Err(load_error)) => rsx! {
                            div { class: AppStyles::dx_lane_error, "Unable to load lanes: {load_error}" }
                        },
                        None => rsx! {
                            div { class: AppStyles::dx_lane_empty, "Loading lanes..." }
                        },
                    }
                    SidebarMenuItem {
                        form {
                            class: AppStyles::dx_lane_create,
                            onsubmit: move |event| {
                                event.prevent_default();
                                let lane_title = title().trim().to_string();
                                if lane_title.is_empty() {
                                    error.set(Some("Lane title is required.".to_string()));
                                    return;
                                }

                                spawn(async move {
                                    let Some(project_id) = selected_project_id() else {
                                        error.set(Some("Select a project before creating a lane.".to_string()));
                                        return;
                                    };
                                    match create_lane_api(project_id, lane_title).await {
                                        Ok(lane) => {
                                            selected_lane_id.set(Some(lane.id.clone()));
                                            selected_lane_status.set(Some(lane.status));
                                            title.set(String::new());
                                            error.set(None);
                                            lanes.restart();
                                        }
                                        Err(create_error) => error.set(Some(create_error.to_string())),
                                    }
                                });
                            },
                            input {
                                aria_label: "New lane title",
                                placeholder: "New lane...",
                                value: "{title}",
                                oninput: move |event| title.set(event.value()),
                            }
                            button { r#type: "submit", "+" }
                        }
                        if let Some(error) = error() {
                            div { class: AppStyles::dx_lane_error, "{error}" }
                        }
                    }
                }
            }
        }
        SidebarGroup {
            SidebarGroupLabel { "Archived" }
            SidebarGroupContent {
                SidebarMenu {
                    match &*lanes.read_unchecked() {
                        Some(Ok(projection)) if !projection.archived.is_empty() => rsx! {
                            for lane in projection.archived.iter() {
                                SidebarMenuItem {
                                    key: "{lane.id}",
                                    LaneButton {
                                        lane: lane.clone(),
                                        selected: selected_lane_id().as_deref() == Some(lane.id.as_str()),
                                        selected_lane_id,
                                        selected_lane_status,
                                    }
                                }
                            }
                        },
                        Some(Ok(_)) => rsx! {
                            div { class: AppStyles::dx_lane_empty, "No archived lanes" }
                        },
                        Some(Err(_)) | None => rsx! {},
                    }
                }
            }
        }
    }
}

#[component]
fn ArchiveLaneButton(
    lane: WorkbenchLane,
    selected_project_id: Signal<Option<String>>,
    mut selected_lane_id: Signal<Option<String>>,
    mut selected_lane_status: Signal<Option<String>>,
    mut lanes_revision: Signal<u64>,
) -> Element {
    let lane_id = lane.id.clone();
    let lane_title = lane.title.clone();
    let attributes = attributes!(button {
        onclick: move |_| {
            let Some(project_id) = selected_project_id() else {
                return;
            };
            let lane_id = lane_id.clone();
            spawn(async move {
                if archive_lane_api(project_id, lane_id.clone()).await.is_ok() {
                    if selected_lane_id().as_deref() == Some(lane_id.as_str()) {
                        selected_lane_id.set(None);
                        selected_lane_status.set(None);
                    }
                    lanes_revision.set(lanes_revision() + 1);
                }
            });
        },
    });

    rsx! {
        SidebarMenuAction {
            show_on_hover: true,
            class: AppStyles::dx_lane_archive_button,
            aria_label: "Archive lane {lane_title}",
            attributes,
            "X"
        }
    }
}

#[component]
fn LaneButton(
    lane: WorkbenchLane,
    selected: bool,
    mut selected_lane_id: Signal<Option<String>>,
    mut selected_lane_status: Signal<Option<String>>,
) -> Element {
    let mut class = AppStyles::dx_lane_button.to_string();
    if selected {
        class = format!("{class} {}", AppStyles::dx_lane_button_active);
    }
    if lane.id == SYSTEM_LANE_ID {
        class = format!("{class} italic");
    }
    let attributes = attributes!(button {
        onclick: move |_| {
            selected_lane_id.set(Some(lane.id.clone()));
            selected_lane_status.set(Some(lane.status.clone()));
        },
    });

    rsx! {
        SidebarMenuButton {
            class,
            attributes,
            DemoIcon {}
            {lane.title}
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
fn ProjectSwitcher(
    mut projects: Resource<ServerFnResult<Vec<ProjectNavItem>>>,
    mut selected_project_id: Signal<Option<String>>,
    mut selected_lane_id: Signal<Option<String>>,
    mut selected_lane_status: Signal<Option<String>>,
) -> Element {
    let mut dialog_open = use_signal(|| false);
    let mut combobox_open = use_signal(|| false);
    let mut project_label = use_signal(String::new);
    let mut project_path = use_signal(String::new);
    let mut create_error = use_signal(|| None::<String>);
    let mut is_creating = use_signal(|| false);

    use_effect(move || {
        if let Some(Ok(items)) = &*projects.read() {
            let current = selected_project_id();

            if items.is_empty() {
                if current.is_some() {
                    selected_project_id.set(None);
                    selected_lane_id.set(None);
                    selected_lane_status.set(None);
                }

                return;
            }

            let needs_selection = current
                .as_ref()
                .is_none_or(|id| !items.iter().any(|project| project.id == *id));

            if needs_selection {
                selected_project_id.set(Some(items[0].id.clone()));
                selected_lane_id.set(None);
                selected_lane_status.set(None);
            }
        }
    });

    rsx! {
        SidebarMenu {
            SidebarMenuItem {
                Combobox::<String> {
                    class: AppStyles::dx_project_combobox,
                    value: Some(selected_project_id.into()),
                    on_open_change: move |open| combobox_open.set(open),
                    on_value_change: move |value: Option<String>| match value.as_deref() {
                        Some(ADD_PROJECT_VALUE) => {
                            create_error.set(None);
                            dialog_open.set(true);
                        }
                        Some(id) => {
                            selected_project_id.set(Some(id.to_string()));
                            selected_lane_id.set(None);
                            selected_lane_status.set(None);
                        },
                        None => {
                            selected_project_id.set(None);
                            selected_lane_id.set(None);
                            selected_lane_status.set(None);
                        },
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
                                selected_project_id.set(Some(project.id));
                                selected_lane_id.set(None);
                                selected_lane_status.set(None);
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
