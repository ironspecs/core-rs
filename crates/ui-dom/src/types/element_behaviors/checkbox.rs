//! Checkbox default-behavior hooks (PR 5, E6-a).
//!
//! This file owns the checkbox's unique commit behavior: the toggle commit that
//! runs after an uncanceled click (formerly the `DomElementOnActivation::Toggle`
//! arm of `RootDom::commit_after_click_after_dispatch`). The checkbox's
//! keyboard/pointer hooks are shared with the other input kinds in
//! `controls.rs`; only the commit is checkbox-specific.

use crate::element_controls::ResolvedElementControl;
use crate::types::{DomError, DomNodeSlotKey, RootDom, RootDomMetricName};

use super::controls::{control_value_event_string, resolved_element_control_for_key};

#[cfg(test)]
use crate::types::{DomEventName, DomPointerUpdate, DomTag, RootDomContext};

/// The checkbox row's click-commit hook: dispatch the input and change events
/// for the toggled checked value and record the committed activation.
pub(crate) fn checkbox_click_commit(
    root: &mut RootDom,
    target_key: DomNodeSlotKey,
) -> Result<bool, DomError> {
    let Some(ResolvedElementControl::Checkbox(control)) =
        resolved_element_control_for_key(root, target_key)
    else {
        return Ok(true);
    };
    if control.disabled {
        return Ok(false);
    }
    let Some(current_value) = control_value_event_string(root, target_key)? else {
        return Ok(false);
    };
    let _ = root.dispatch_input_event_to_target_key(target_key, current_value.clone())?;
    let _ = root.dispatch_change_event_to_target_key(target_key, current_value)?;
    root.increment_metric(RootDomMetricName::ActivationCommitted);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkbox_click_commit_dispatches_input_and_change() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let checkbox = root.create_element(DomTag::Input).expect("checkbox");
        root.append_child(root.root, checkbox).expect("append");
        root.set_attr_str(checkbox, "type", "checkbox")
            .expect("type");

        let committed = checkbox_click_commit(&mut root, checkbox).expect("commit");
        assert!(committed);

        let mut world = bevy::prelude::World::new();
        let drained = root
            .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("drain");
        assert_eq!(
            drained.iter().map(|event| event.name).collect::<Vec<_>>(),
            vec![DomEventName::Input, DomEventName::Change]
        );
    }
}
