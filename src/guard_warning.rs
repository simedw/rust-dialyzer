use crate::{binder::WarningInfo, program::NodeRef};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpaqueSides {
    Left,
    Right,
    Both,
}
impl OpaqueSides {
    pub fn positions(self) -> &'static [usize] {
        match self {
            Self::Left => &[1],
            Self::Right => &[2],
            Self::Both => &[1, 2],
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpaqueGuardMessage {
    Arguments {
        function: String,
        args: String,
    },
    Infix {
        left: String,
        operator: String,
        right: String,
        positions: OpaqueSides,
    },
}
#[derive(Clone, Debug)]
pub struct GuardOpaque {
    pub guard: NodeRef,
    pub info: WarningInfo,
    pub message: OpaqueGuardMessage,
}
impl PartialEq for GuardOpaque {
    fn eq(&self, rhs: &Self) -> bool {
        self.info == rhs.info && self.message == rhs.message
    }
}
impl Eq for GuardOpaque {}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GuardWarning {
    OpaqueGuard(GuardOpaque),
}

impl GuardWarning {
    pub fn native_warning(&self) -> crate::warning_payload::NativeWarning {
        use crate::{
            lattice::NativeValue,
            warning_payload::{NativeWarning, string},
        };
        match self {
            Self::OpaqueGuard(warning) => {
                let details = match &warning.message {
                    OpaqueGuardMessage::Arguments { function, args } => {
                        vec![NativeValue::Atom(function.clone()), string(args)]
                    }
                    OpaqueGuardMessage::Infix {
                        left,
                        operator,
                        right,
                        positions,
                    } => vec![
                        string(left),
                        string(operator),
                        string(right),
                        NativeValue::List(
                            positions
                                .positions()
                                .iter()
                                .map(|&p| NativeValue::Integer(p.into()))
                                .collect(),
                        ),
                    ],
                };
                NativeWarning::from_value(
                    "warn_opaque",
                    &warning.info,
                    &NativeValue::Tuple(vec![
                        NativeValue::Atom("opaque_guard".into()),
                        NativeValue::List(details),
                    ]),
                )
            }
        }
    }
}
