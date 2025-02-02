use dada_ir::storage_mode::Joint;

use crate::machine::{Permission, PermissionData, ValidPermissionData};

use super::Stepper;

impl Stepper<'_> {
    /// Revokes the given permission, recording the current PC as the "reason".
    #[tracing::instrument(level = "Debug", skip(self))]
    pub(super) fn revoke(&mut self, permission: Permission) {
        let pc = self.machine.opt_pc();
        let p = std::mem::replace(&mut self.machine[permission], PermissionData::Expired(pc));

        if let PermissionData::Valid(ValidPermissionData { tenants, .. }) = p {
            for tenant in tenants {
                self.revoke(tenant);
            }
        }
    }

    /// True if the permission `p` is currently sharing access to the object's
    /// fields. This is true if `permission` is a joint permission, but it's
    /// also true if it's a unique permission that is leased by a joint permission.
    fn is_sharing_access(&self, permission: Permission) -> bool {
        let Some(valid) = self.machine[permission].valid() else {
            return false;
        };

        if let Joint::Yes = valid.joint {
            return true;
        }

        valid.tenants.iter().any(|p| self.is_sharing_access(*p))
    }

    #[tracing::instrument(level = "Debug", skip(self))]
    pub(super) fn revoke_tenants(&mut self, permission: Permission) {
        // Temporarily swap out the data for `permission`...
        let mut p = std::mem::replace(&mut self.machine[permission], PermissionData::Expired(None));

        // Cancel all the tenants and clear the list
        if let PermissionData::Valid(ValidPermissionData { tenants, .. }) = &mut p {
            for tenant in std::mem::take(tenants) {
                self.revoke(tenant);
            }
        }

        // Put the (modified) data for `p` back
        self.machine[permission] = p;
    }

    /// Revoke any tenant of `permission` that is not currently
    /// sharing access to the object.
    ///
    /// Used when the object is read through `permission` (or a write
    /// to an atomic field).
    ///
    /// (There should be at most one such tenant.)
    #[tracing::instrument(level = "Debug", skip(self))]
    pub(super) fn revoke_exclusive_tenants(&mut self, permission: Permission) {
        // Temporarily swap out the data for `permission`...
        let mut p = std::mem::replace(&mut self.machine[permission], PermissionData::Expired(None));

        // Cancel all the exclusive tenants and remove them from the list
        if let PermissionData::Valid(ValidPermissionData { tenants, .. }) = &mut p {
            tenants.retain(|&tenant| {
                if !self.is_sharing_access(tenant) {
                    self.revoke(tenant);
                    false
                } else {
                    true
                }
            });
        }

        // Put the (modified) data for `p` back
        self.machine[permission] = p;
    }
}
