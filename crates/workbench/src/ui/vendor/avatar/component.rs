use dioxus::prelude::*;
use dioxus_primitives::avatar::{self, AvatarState};

#[css_module("/src/ui/vendor/avatar/style.css")]
struct Styles;

#[derive(Clone, Copy, PartialEq, Default)]
pub enum AvatarImageSize {
    #[default]
    Small,
    Medium,
    Large,
}

#[derive(Clone, Copy, PartialEq, Default)]
pub enum AvatarShape {
    #[default]
    Circle,
    Rounded,
}

/// The props for the [`Avatar`] component.
#[derive(Props, Clone, PartialEq)]
pub struct AvatarProps {
    /// The image source URL.
    pub src: String,

    /// The image alt text.
    #[props(default)]
    pub alt: String,

    /// Callback when image loads successfully.
    #[props(default)]
    pub on_load: Option<EventHandler<()>>,

    /// Callback when image fails to load.
    #[props(default)]
    pub on_error: Option<EventHandler<()>>,

    /// Callback when the avatar state changes.
    #[props(default)]
    pub on_state_change: Option<EventHandler<AvatarState>>,

    #[props(default)]
    pub size: AvatarImageSize,

    #[props(default)]
    pub shape: AvatarShape,

    /// Additional attributes for the avatar element.
    #[props(extends = GlobalAttributes)]
    pub attributes: Vec<Attribute>,

    /// The fallback content shown while the image is loading or if it fails to load.
    pub children: Element,
}

impl AvatarImageSize {
    fn to_class(self) -> &'static str {
        match self {
            AvatarImageSize::Small => Styles::dx_avatar_sm.inner,
            AvatarImageSize::Medium => Styles::dx_avatar_md.inner,
            AvatarImageSize::Large => Styles::dx_avatar_lg.inner,
        }
    }
}

impl AvatarShape {
    fn to_class(self) -> &'static str {
        match self {
            AvatarShape::Circle => Styles::dx_avatar_circle.inner,
            AvatarShape::Rounded => Styles::dx_avatar_rounded.inner,
        }
    }
}

#[component]
pub fn Avatar(props: AvatarProps) -> Element {
    let class = format!(
        "{} {} {}",
        Styles::dx_avatar,
        props.size.to_class(),
        props.shape.to_class()
    );

    rsx! {
        avatar::Avatar {
            class,
            on_load: props.on_load,
            on_error: props.on_error,
            on_state_change: props.on_state_change,
            attributes: props.attributes,
            avatar::AvatarImage {
                class: Styles::dx_avatar_image,
                src: props.src,
                alt: props.alt,
                draggable: "false",
            }
            avatar::AvatarFallback {
                class: Styles::dx_avatar_fallback,
                {props.children}
            }
        }
    }
}
