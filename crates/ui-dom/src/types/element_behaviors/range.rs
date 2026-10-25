//! Range default-behavior hooks (PR 5, E6-a).
//!
//! This file owns the range control kind's pointer-drag behavior: the
//! retained `ActiveRangeDragSession` and the drag-continue/commit lifecycle
//! (formerly `RootDom::apply_default_range_drag_for_point` plus the range-drag
//! trio `start_range_drag_session`, `mark_active_range_drag_dirty`, and
//! `finish_range_drag_change_if_dirty`). The range's keyboard and pointer-down
//! hooks are shared with the other input kinds in `controls.rs`.

use crate::element_controls::ResolvedElementControl;
use crate::types::primitives::PointPx;
use crate::types::{DomError, DomNodeSlotKey, RootDom};

use super::activation::ActiveRangeDragSession;
use super::controls::{
    control_value_event_string, range_value_string_for_point, resolved_element_control_for_key,
    set_control_value_and_dispatch_input,
};

#[cfg(test)]
use crate::types::{DomEventName, DomPointerUpdate, DomTag, RootDomContext};

/// Continues an active range drag for one pointer move (the range's
/// pointer-move seam, called from `RootDom::dispatch_pointer_move_for_update`).
pub(crate) fn apply_default_range_drag_for_point(
    root: &mut RootDom,
    point: PointPx,
) -> Result<(), DomError> {
    let Some(session) = root.active_range_drag_session.clone() else {
        return Ok(());
    };
    let Some(ResolvedElementControl::Range(control)) =
        resolved_element_control_for_key(root, session.target_key)
    else {
        root.active_range_drag_session = None;
        return Ok(());
    };
    if control.disabled {
        root.active_range_drag_session = None;
        return Ok(());
    }
    let subtree_point = range_drag_subtree_point(&session, point);
    let next_value =
        range_value_string_for_point(root, session.target_key, subtree_point, control)?;
    let changed = set_control_value_and_dispatch_input(root, session.target_key, next_value)?;
    mark_active_range_drag_dirty(root, session.target_key, changed);
    Ok(())
}

pub(crate) fn start_range_drag_session(
    root: &mut RootDom,
    target_key: DomNodeSlotKey,
    root_point: PointPx,
    subtree_point: PointPx,
) -> Result<(), DomError> {
    let focus_entry_value =
        control_value_event_string(root, target_key)?.unwrap_or_else(|| String::from("0"));
    root.active_range_drag_session = Some(ActiveRangeDragSession {
        target_key,
        root_to_subtree_point_offset: PointPx {
            x_px: subtree_point.x_px - root_point.x_px,
            y_px: subtree_point.y_px - root_point.y_px,
        },
        focus_entry_value,
        dirty: false,
    });
    Ok(())
}

pub(crate) fn mark_active_range_drag_dirty(
    root: &mut RootDom,
    target_key: DomNodeSlotKey,
    changed: bool,
) {
    if !changed {
        return;
    }
    if let Some(session) = root.active_range_drag_session.as_mut()
        && session.target_key == target_key
    {
        session.dirty = true;
    }
}

pub(crate) fn finish_range_drag_change_if_dirty(root: &mut RootDom) -> Result<(), DomError> {
    let Some(session) = root.active_range_drag_session.take() else {
        return Ok(());
    };
    if !session.dirty {
        return Ok(());
    }
    let Some(current_value) = control_value_event_string(root, session.target_key)? else {
        return Ok(());
    };
    if current_value != session.focus_entry_value {
        let _ = root.dispatch_change_event_to_target_key(session.target_key, current_value)?;
    }
    Ok(())
}

fn range_drag_subtree_point(session: &ActiveRangeDragSession, root_point: PointPx) -> PointPx {
    PointPx {
        x_px: root_point.x_px + session.root_to_subtree_point_offset.x_px,
        y_px: root_point.y_px + session.root_to_subtree_point_offset.y_px,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_drag_session_finishes_a_dirty_change_with_a_change_event() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let range = root.create_element(DomTag::Input).expect("range");
        root.append_child(root.root, range).expect("append");
        root.set_attr_str(range, "type", "range").expect("type");
        root.set_attr_str(range, "min", "0").expect("min");
        root.set_attr_str(range, "max", "20").expect("max");
        root.set_attr_str(range, "value", "10").expect("value");

        start_range_drag_session(
            &mut root,
            range,
            PointPx {
                x_px: 0.0,
                y_px: 0.0,
            },
            PointPx {
                x_px: 0.0,
                y_px: 0.0,
            },
        )
        .expect("start drag");
        mark_active_range_drag_dirty(&mut root, range, true);
        set_control_value_and_dispatch_input(&mut root, range, "11".to_string())
            .expect("set value");
        finish_range_drag_change_if_dirty(&mut root).expect("finish drag");
        assert!(root.active_range_drag_session.is_none());

        let mut world = bevy::prelude::World::new();
        let drained = root
            .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("drain");
        assert_eq!(
            drained.iter().map(|event| event.name).collect::<Vec<_>>(),
            vec![DomEventName::Input, DomEventName::Change]
        );
    }

    #[test]
    fn clean_range_drag_session_finishes_without_a_change_event() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let range = root.create_element(DomTag::Input).expect("range");
        root.append_child(root.root, range).expect("append");
        root.set_attr_str(range, "type", "range").expect("type");
        root.set_attr_str(range, "value", "10").expect("value");

        start_range_drag_session(
            &mut root,
            range,
            PointPx {
                x_px: 0.0,
                y_px: 0.0,
            },
            PointPx {
                x_px: 0.0,
                y_px: 0.0,
            },
        )
        .expect("start drag");
        finish_range_drag_change_if_dirty(&mut root).expect("finish drag");
        assert!(root.active_range_drag_session.is_none());

        let mut world = bevy::prelude::World::new();
        let drained = root
            .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("drain");
        assert!(drained.is_empty());
    }
}
