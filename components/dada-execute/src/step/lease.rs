use dada_ir::{
    code::bir,
    storage_mode::{Joint, Leased},
};

use crate::machine::{ValidPermissionData, Value};

use super::{traversal::ObjectTraversal, Stepper};

impl Stepper<'_> {
    /// Leasing an object creates a new permission that remains valid so long as the
    /// original reference is not "reasserted" (used again).
    ///
    /// # Examples
    ///
    /// Creates a shared point:
    ///
    /// ```notrust
    /// p = Point(22, 44).share
    /// ```
    ///
    /// # Invariants
    ///
    /// The following invariants are maintained:
    ///
    /// * Results in a leased value that refers to the same object as `place`
    /// * Preserves the sharing properties of `place`:
    ///   * If `place` is jointly accessible, result will be jointly accessible
    ///   * If `place` is exclusive, result will be exclusive
    /// * `place` remains valid and unchanged; asserting `place` or invalidating it may invalidate the result
    pub(super) fn lease_place(
        &mut self,
        table: &bir::Tables,
        place: bir::Place,
    ) -> eyre::Result<Value> {
        let object_traversal = self.traverse_to_object(table, place)?;
        self.lease_traversal(object_traversal)
    }

    #[tracing::instrument(level = "Debug", skip(self))]
    pub(super) fn lease_traversal(
        &mut self,
        object_traversal: ObjectTraversal,
    ) -> eyre::Result<Value> {
        let ObjectTraversal {
            object,
            accumulated_permissions,
        } = object_traversal;

        // XXX assert the traversed edges are valid for lease (possibly joint lease)

        // The last traversed permission is the one that led to the object
        // (and there must be one, because you can't reach an object without
        // traversing at least one permision).
        let last_permission = *accumulated_permissions.traversed.last().unwrap();

        // Special case: If last permission is already a shared lease, we can just duplicate it
        //
        // # Example
        //
        // ```notrust
        // a ----> [ Obj ]
        //   ===== [  f  ] --shared---> b
        //     |           ============ duplicate this permission
        //   can be any
        //   permission(s)
        // ```
        //
        // # Discussion
        //
        // Maintains our invariants:
        //
        // * Result is leased.
        // * Preserves sharing properties.
        // * `place` is not altered.
        if let ValidPermissionData {
            joint: Joint::Yes,
            leased: Leased::Yes,
            ..
        } = self.machine[last_permission].assert_valid()
        {
            return Ok(Value {
                object,
                permission: last_permission,
            });
        }

        // Otherwise, allocate a new lease permission; if we have exclusive
        // access along this path, make it exclusive, but joint otherwise.
        //
        // ## Examples
        //
        // In each case, we share `a.f`...
        //
        // ```notrust
        // a -my-> [ Obj ]
        //         [  f  ] --my------> b
        //                   :
        //                   : tenant
        //                   v
        //                 --shared--> b
        //                 =========== resulting permission
        // ```
        //
        // ```notrust
        // a -my-> [ Obj ]
        //         [  f  ] --leased---> b
        //                   :
        //                   : tenant
        //                   v
        //                 --shared--> b
        //                 =========== resulting permission
        // ```
        //
        // ```notrust
        // a -our-> [ Obj ]
        //          [  f  ] --leased---> b
        //                    :
        //                    : tenant
        //                    v
        //                  --shared--> b
        //                  =========== resulting permission
        // ```
        //
        // In each case, reasserting `a.f` *may* invalidate the resulting
        // permission.
        //
        // # Discussion
        //
        // Maintains our invariants:
        //
        // * Result is leased.
        // * We create a shared lease if the input is shared, preserving sharing properties.
        // * Permissions for `place` are never altered.
        let permission = self.new_tenant_permission(accumulated_permissions.joint, last_permission);

        Ok(Value { object, permission })
    }
}
