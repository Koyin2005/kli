use crate::define_id;

define_id!(DefId);
impl DefId {
    pub const ROOT: Self = Self(0);
}
