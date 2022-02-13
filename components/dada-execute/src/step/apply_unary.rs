use dada_ir::{
    code::{bir, validated::op::Op},
    error,
};

use crate::{
    error::DiagnosticBuilderExt,
    machine::op::MachineOpExt,
    machine::{Object, ObjectData, Value},
};

use super::Stepper;

impl Stepper<'_> {
    pub(super) fn apply_unary(
        &mut self,
        expr: bir::Expr,
        op: Op,
        rhs: Object,
    ) -> eyre::Result<Value> {
        let op_error = || {
            let span = self.span_from_bir(expr);
            Err(error!(
                span,
                "cannot apply operator {} to {}",
                op,
                self.machine[rhs].kind_str(self.db),
            )
            .eyre(self.db))
        };
        // let div_zero_error = || {
        //     let span = self.span_from_bir(expr);
        //     Err(error!(span, "divide by zero").eyre(self.db))
        // };
        // let overflow_error = || {
        //     let span = self.span_from_bir(expr);
        //     Err(error!(span, "overflow").eyre(self.db))
        // };
        match (op, &self.machine[rhs]) {
            (Op::Minus, &ObjectData::Int(rhs)) => Ok(self.machine.our_value(-rhs)),
            _ => op_error(),
        }
    }
}
