use crate::{
    core::{ErlangTerm, TermId},
    form_error::{FormError, ReasonBuilder},
    native_error::NativeTransferError,
    raw_literal::RawLiteral,
};
#[derive(Clone, Debug)]
pub struct BehaviourAttribute {
    pub payload: RawLiteral,
    pub annotations: RawLiteral,
}
#[derive(Clone, Debug)]
pub struct BehaviourOccurrence {
    pub name: RawLiteral,
    pub location: RawLiteral,
}
fn head(annotations: &RawLiteral) -> Result<RawLiteral, FormError> {
    let mut id = annotations.raw().id();
    loop {
        match &annotations.raw().arena().terms[id.0] {
            ErlangTerm::List { items, tail } => {
                if let Some(&id) = items.first() {
                    return annotations.at(id).map_err(FormError::Transport);
                }
                id = *tail;
            }
            _ => return Err(FormError::Native(NativeTransferError::Badarg)),
        }
    }
}
fn parts(payload: &RawLiteral) -> (Vec<TermId>, TermId) {
    let mut items = Vec::new();
    let mut tail = payload.raw().id();
    while let ErlangTerm::List {
        items: xs,
        tail: next,
    } = &payload.raw().arena().terms[tail.0]
    {
        items.extend(xs);
        tail = *next;
    }
    (items, tail)
}
pub fn collect<'a>(
    attributes: impl IntoIterator<Item = &'a BehaviourAttribute>,
) -> Result<Vec<BehaviourOccurrence>, FormError> {
    let attributes = attributes
        .into_iter()
        .map(|attribute| Ok((attribute, head(&attribute.annotations)?)))
        .collect::<Result<Vec<_>, FormError>>()?;
    // lists:append returns its last operand directly; preceding operands require proper lists.
    for (attribute, _) in attributes.iter().take(attributes.len().saturating_sub(1)) {
        let (_, tail) = parts(&attribute.payload);
        if !matches!(
            attribute.payload.raw().arena().terms[tail.0],
            ErlangTerm::Nil
        ) {
            return Err(FormError::Native(NativeTransferError::Badarg));
        }
    }
    let mut result = Vec::new();
    for (attribute, location) in attributes {
        let (items, tail) = parts(&attribute.payload);
        for id in items {
            result.push(BehaviourOccurrence {
                name: attribute.payload.at(id).map_err(FormError::Transport)?,
                location: location.clone(),
            });
        }
        if !matches!(
            attribute.payload.raw().arena().terms[tail.0],
            ErlangTerm::Nil
        ) {
            let mut b = ReasonBuilder::new();
            let tag = b.atom("bad_generator");
            let value = b.copy(
                attribute
                    .payload
                    .at(tail)
                    .map_err(FormError::Transport)?
                    .raw(),
            );
            let root = b.tuple(vec![tag, value]);
            return Err(FormError::ErrorReason(b.finish(root)));
        }
    }
    Ok(result)
}
