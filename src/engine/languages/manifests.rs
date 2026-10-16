/// Typed framework rule data loaded from `.forge/frameworks/*.toml`, `.yaml`, or `.yml`.
pub mod framework;

pub use framework::{
    ClassHierarchyReceiverRule, DiCallRule, FrameworkManifest, FrameworkManifestValue,
    RouteCallRule, RouteDecoratorRule,
};
