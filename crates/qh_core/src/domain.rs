use qh_macros::define_id;

pub mod context;
pub mod message;

define_id!(pub ModelId, String);
define_id!(pub AdapterId, String);

define_id!(pub PluginId, i64); // TODO domain.rs
define_id!(pub SessionId, i64);// TODO domain.rs