//! Shared inherited-style context and ancestor climber for retained visitors.
//!
//! This module owns one upward seed walk that retained visitors can reuse at
//! their root boundary. The climber is intentionally small and strict:
//!
//! - it walks upward from one subtree root
//! - it gathers only the inherited values currently shared by style/layout
//! - it stops as soon as all tracked inherited values are known
//! - it does not recompute per-node maps or allocate traversal history

use crate::domains::text_layout::DEFAULT_TEXT_FONT_SIZE_PX;
use crate::types::dom_node::{DomNode, DomNodeSlotKey};
use crate::types::{
    AuthoredLength, DomAttribute, DomAttributeValue, DomError, DomOverflowWrap, DomTextAlign,
    DomTextDirection, DomWhiteSpace, DomWordBreak, RootDom, overflow_wrap_from_attr_value,
    parse_authored_length, resolve_line_height_px, text_align_from_attr_value,
    text_direction_from_attr_value, white_space_from_attr_value, word_break_from_attr_value,
};

#[cfg(test)]
use crate::types::{DomTag, RootDomContext, RootDomContextConfig, parse_css_color_rgba};

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct InheritedStyleContext {
    pub(crate) color: Option<DomAttributeValue>,
    pub(crate) font_size_px: Option<f32>,
    pub(crate) line_height_px: Option<f32>,
    pub(crate) text_align: Option<DomTextAlign>,
    pub(crate) text_direction: Option<DomTextDirection>,
    pub(crate) white_space: Option<DomWhiteSpace>,
    pub(crate) overflow_wrap: Option<DomOverflowWrap>,
    pub(crate) word_break: Option<DomWordBreak>,
}

impl InheritedStyleContext {
    pub(crate) fn effective_font_size_px(&self) -> f32 {
        self.font_size_px.unwrap_or(DEFAULT_TEXT_FONT_SIZE_PX)
    }

    pub(crate) fn effective_white_space(&self) -> DomWhiteSpace {
        self.white_space.unwrap_or_default()
    }

    pub(crate) fn effective_overflow_wrap(&self) -> DomOverflowWrap {
        self.overflow_wrap.unwrap_or_default()
    }

    pub(crate) fn effective_word_break(&self) -> DomWordBreak {
        self.word_break.unwrap_or_default()
    }

    pub(crate) fn effective_line_height_px(&self) -> Option<f32> {
        self.line_height_px
    }

    pub(crate) fn effective_text_align(&self) -> DomTextAlign {
        self.text_align.unwrap_or_default()
    }

    pub(crate) fn effective_text_direction(&self) -> DomTextDirection {
        self.text_direction.unwrap_or_default()
    }
}

pub(crate) fn inherited_style_context_from_ancestors(
    root_dom: &RootDom,
    start_key: DomNodeSlotKey,
) -> Result<InheritedStyleContext, DomError> {
    let mut context = InheritedStyleContext::default();
    let mut pending_font_size_scale = 1.0;
    let mut pending_line_height_value = None;
    let mut current_parent = root_dom
        .node(start_key)
        .ok_or(DomError::InvalidNodeKey)?
        .parent_key();
    while let Some(parent_key) = current_parent {
        let parent = root_dom.node(parent_key).ok_or(DomError::InvalidNodeKey)?;
        capture_color_from_node(parent, &mut context);
        capture_inherited_font_size_from_node(parent, &mut context, &mut pending_font_size_scale)?;
        capture_line_height_value_from_node(parent, &mut pending_line_height_value);
        capture_text_align_from_node(parent, &mut context)?;
        capture_text_direction_from_node(parent, &mut context)?;
        capture_white_space_from_node(parent, &mut context)?;
        capture_overflow_wrap_from_node(parent, &mut context)?;
        capture_word_break_from_node(parent, &mut context)?;
        if inherited_context_is_complete(&context, pending_line_height_value.is_some()) {
            break;
        }
        current_parent = parent.parent_key();
    }
    if context.font_size_px.is_none() && pending_font_size_scale.to_bits() != 1.0f32.to_bits() {
        context.font_size_px = Some(DEFAULT_TEXT_FONT_SIZE_PX * pending_font_size_scale);
    }
    if let Some(value) = pending_line_height_value {
        context.line_height_px = resolve_line_height_px(&value, context.effective_font_size_px());
    }
    Ok(context)
}

pub(crate) fn inherited_context_from_node(
    node: &DomNode,
    inherited_context: &InheritedStyleContext,
) -> Result<InheritedStyleContext, DomError> {
    let font_size_px = match node.presented_attr(&DomAttribute::FontSize) {
        Some(value) => Some(
            resolve_font_size_px_from_value(value, inherited_context.effective_font_size_px())
                .ok_or(DomError::InvalidAttributeValue)?,
        ),
        None => inherited_context.font_size_px,
    };
    let effective_font_size_px = font_size_px.unwrap_or(DEFAULT_TEXT_FONT_SIZE_PX);
    Ok(InheritedStyleContext {
        color: node
            .presented_attr(&inherited_color_attribute())
            .cloned()
            .or_else(|| inherited_context.color.clone()),
        font_size_px,
        line_height_px: match node.presented_attr(&DomAttribute::LineHeight) {
            Some(value) => resolve_line_height_px(value, effective_font_size_px),
            None => inherited_context.line_height_px,
        },
        text_align: match node.presented_attr(&DomAttribute::TextAlign) {
            Some(value) => Some(text_align_from_attr_value(value)?),
            None => inherited_context.text_align,
        },
        text_direction: match presented_text_direction_value(node) {
            Some(value) => Some(text_direction_from_attr_value(value)?),
            None => inherited_context.text_direction,
        },
        white_space: match node.presented_attr(&DomAttribute::WhiteSpace) {
            Some(value) => Some(white_space_from_attr_value(value)?),
            None => inherited_context.white_space,
        },
        overflow_wrap: match node.presented_attr(&DomAttribute::OverflowWrap) {
            Some(value) => Some(overflow_wrap_from_attr_value(value)?),
            None => inherited_context.overflow_wrap,
        },
        word_break: match node.presented_attr(&DomAttribute::WordBreak) {
            Some(value) => Some(word_break_from_attr_value(value)?),
            None => inherited_context.word_break,
        },
    })
}

pub(crate) fn inherited_color_attribute() -> DomAttribute {
    DomAttribute::Custom("color".to_string())
}

fn inherited_context_is_complete(
    context: &InheritedStyleContext,
    line_height_value_is_known: bool,
) -> bool {
    context.color.is_some()
        && context.font_size_px.is_some()
        && line_height_value_is_known
        && context.text_align.is_some()
        && context.text_direction.is_some()
        && context.white_space.is_some()
        && context.overflow_wrap.is_some()
        && context.word_break.is_some()
}

fn capture_color_from_node(node: &DomNode, context: &mut InheritedStyleContext) {
    if context.color.is_some() {
        return;
    }
    let Some(value) = node.presented_attr(&inherited_color_attribute()) else {
        return;
    };
    context.color = Some(value.clone());
}

fn capture_white_space_from_node(
    node: &DomNode,
    context: &mut InheritedStyleContext,
) -> Result<(), DomError> {
    if context.white_space.is_some() {
        return Ok(());
    }
    let Some(value) = node.presented_attr(&DomAttribute::WhiteSpace) else {
        return Ok(());
    };
    context.white_space = Some(white_space_from_attr_value(value)?);
    Ok(())
}

fn capture_text_align_from_node(
    node: &DomNode,
    context: &mut InheritedStyleContext,
) -> Result<(), DomError> {
    if context.text_align.is_some() {
        return Ok(());
    }
    let Some(value) = node.presented_attr(&DomAttribute::TextAlign) else {
        return Ok(());
    };
    context.text_align = Some(text_align_from_attr_value(value)?);
    Ok(())
}

fn capture_text_direction_from_node(
    node: &DomNode,
    context: &mut InheritedStyleContext,
) -> Result<(), DomError> {
    if context.text_direction.is_some() {
        return Ok(());
    }
    let Some(value) = presented_text_direction_value(node) else {
        return Ok(());
    };
    context.text_direction = Some(text_direction_from_attr_value(value)?);
    Ok(())
}

fn capture_overflow_wrap_from_node(
    node: &DomNode,
    context: &mut InheritedStyleContext,
) -> Result<(), DomError> {
    if context.overflow_wrap.is_some() {
        return Ok(());
    }
    let Some(value) = node.presented_attr(&DomAttribute::OverflowWrap) else {
        return Ok(());
    };
    context.overflow_wrap = Some(overflow_wrap_from_attr_value(value)?);
    Ok(())
}

fn capture_word_break_from_node(
    node: &DomNode,
    context: &mut InheritedStyleContext,
) -> Result<(), DomError> {
    if context.word_break.is_some() {
        return Ok(());
    }
    let Some(value) = node.presented_attr(&DomAttribute::WordBreak) else {
        return Ok(());
    };
    context.word_break = Some(word_break_from_attr_value(value)?);
    Ok(())
}

fn presented_text_direction_value(node: &DomNode) -> Option<&DomAttributeValue> {
    node.presented_attr(&DomAttribute::Direction)
        .or_else(|| node.presented_attr(&DomAttribute::Dir))
}

fn capture_line_height_value_from_node(
    node: &DomNode,
    pending_line_height_value: &mut Option<DomAttributeValue>,
) {
    if pending_line_height_value.is_some() {
        return;
    }
    let Some(value) = node.presented_attr(&DomAttribute::LineHeight) else {
        return;
    };
    *pending_line_height_value = Some(value.clone());
}

fn capture_inherited_font_size_from_node(
    node: &DomNode,
    context: &mut InheritedStyleContext,
    pending_font_size_scale: &mut f32,
) -> Result<(), DomError> {
    if context.font_size_px.is_some() {
        return Ok(());
    }
    let Some(value) = node.presented_attr(&DomAttribute::FontSize) else {
        return Ok(());
    };
    match parse_font_size_step(value) {
        Some(FontSizeStep::Absolute(px)) => {
            context.font_size_px = Some(px * *pending_font_size_scale);
        }
        Some(FontSizeStep::Scale(em_scale)) => {
            *pending_font_size_scale *= em_scale;
        }
        None => return Err(DomError::InvalidAttributeValue),
    }
    Ok(())
}

enum FontSizeStep {
    Absolute(f32),
    Scale(f32),
}

fn parse_font_size_step(value: &DomAttributeValue) -> Option<FontSizeStep> {
    match value {
        DomAttributeValue::Float(value) if value.is_finite() => {
            Some(FontSizeStep::Absolute(*value))
        }
        DomAttributeValue::Float(_) | DomAttributeValue::Bool(_) | DomAttributeValue::Color(_) => {
            None
        }
        DomAttributeValue::Integer(value) => {
            Some(FontSizeStep::Absolute(crate::numeric::i64_to_f32(*value)))
        }
        DomAttributeValue::String(value) => match parse_authored_length(value)? {
            AuthoredLength::Px(px) => Some(FontSizeStep::Absolute(px)),
            AuthoredLength::Em(scale) => Some(FontSizeStep::Scale(scale)),
            AuthoredLength::Unset => None,
        },
    }
}

fn resolve_font_size_px_from_value(value: &DomAttributeValue, font_size_px: f32) -> Option<f32> {
    match parse_font_size_step(value)? {
        FontSizeStep::Absolute(px) => Some(px),
        FontSizeStep::Scale(scale) => Some(font_size_px * scale),
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn color(value: &str) -> DomAttributeValue {
        DomAttributeValue::Color(parse_css_color_rgba(value).expect("test color should parse"))
    }

    #[test]
    fn climber_prefers_nearest_ancestor_values() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context);
        let far = root.create_element(DomTag::Div).expect("far");
        let near = root.create_element(DomTag::Div).expect("near");
        let start = root.create_element(DomTag::Div).expect("start");
        root.append_child(root.root, far).expect("append far");
        root.append_child(far, near).expect("append near");
        root.append_child(near, start).expect("append start");
        root.set_attr(
            far,
            DomAttribute::FontSize,
            Some(DomAttributeValue::String("12px".to_string())),
        )
        .expect("far font-size");
        root.set_attr(
            far,
            DomAttribute::Custom("color".to_string()),
            Some(DomAttributeValue::String("#111111".to_string())),
        )
        .expect("far color");
        root.set_attr(
            near,
            DomAttribute::FontSize,
            Some(DomAttributeValue::String("18px".to_string())),
        )
        .expect("near font-size");
        root.set_attr(
            near,
            DomAttribute::Custom("color".to_string()),
            Some(DomAttributeValue::String("#222222".to_string())),
        )
        .expect("near color");

        let inherited = inherited_style_context_from_ancestors(&root, start).expect("climb");

        assert_eq!(inherited.font_size_px, Some(18.0));
        assert_eq!(inherited.color, Some(color("#222222")));
    }

    #[test]
    fn climber_uses_default_font_size_when_none_is_found() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context);
        let parent = root.create_element(DomTag::Div).expect("parent");
        let start = root.create_element(DomTag::Div).expect("start");
        root.append_child(root.root, parent).expect("append parent");
        root.append_child(parent, start).expect("append start");
        root.set_attr(
            parent,
            DomAttribute::Custom("color".to_string()),
            Some(DomAttributeValue::String("#abcdef".to_string())),
        )
        .expect("color");

        let inherited = inherited_style_context_from_ancestors(&root, start).expect("climb");

        assert_eq!(inherited.font_size_px, None);
        assert_eq!(inherited.effective_font_size_px(), 14.0);
        assert_eq!(inherited.color, Some(color("#abcdef")));
    }

    #[test]
    fn climber_resolves_nearest_font_size_em_chain_against_found_px_base() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context);
        let far = root.create_element(DomTag::Div).expect("far");
        let near = root.create_element(DomTag::Div).expect("near");
        let start = root.create_element(DomTag::Div).expect("start");
        root.append_child(root.root, far).expect("append far");
        root.append_child(far, near).expect("append near");
        root.append_child(near, start).expect("append start");
        root.set_attr(
            far,
            DomAttribute::FontSize,
            Some(DomAttributeValue::String("10px".to_string())),
        )
        .expect("far font-size");
        root.set_attr(
            near,
            DomAttribute::FontSize,
            Some(DomAttributeValue::String("1.5em".to_string())),
        )
        .expect("near font-size");

        let inherited = inherited_style_context_from_ancestors(&root, start).expect("climb");

        assert_eq!(inherited.font_size_px, Some(15.0));
    }

    #[test]
    fn climber_resolves_nearest_line_height_after_font_size_seed() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context);
        let far = root.create_element(DomTag::Div).expect("far");
        let near = root.create_element(DomTag::Div).expect("near");
        let start = root.create_element(DomTag::Div).expect("start");
        root.append_child(root.root, far).expect("append far");
        root.append_child(far, near).expect("append near");
        root.append_child(near, start).expect("append start");
        root.set_attr(
            far,
            DomAttribute::FontSize,
            Some(DomAttributeValue::String("10px".to_string())),
        )
        .expect("far font-size");
        root.set_attr(
            near,
            DomAttribute::LineHeight,
            Some(DomAttributeValue::String("2".to_string())),
        )
        .expect("near line-height");

        let inherited = inherited_style_context_from_ancestors(&root, start).expect("climb");

        assert_eq!(inherited.font_size_px, Some(10.0));
        assert_eq!(inherited.line_height_px, Some(20.0));
    }

    #[test]
    fn climber_prefers_nearest_text_align() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context);
        let far = root.create_element(DomTag::Div).expect("far");
        let near = root.create_element(DomTag::Div).expect("near");
        let start = root.create_element(DomTag::Div).expect("start");
        root.append_child(root.root, far).expect("append far");
        root.append_child(far, near).expect("append near");
        root.append_child(near, start).expect("append start");
        root.set_attr(
            far,
            DomAttribute::TextAlign,
            Some(DomAttributeValue::String("center".to_string())),
        )
        .expect("far text-align");
        root.set_attr(
            near,
            DomAttribute::TextAlign,
            Some(DomAttributeValue::String("right".to_string())),
        )
        .expect("near text-align");

        let inherited = inherited_style_context_from_ancestors(&root, start).expect("climb");

        assert_eq!(inherited.effective_text_align(), DomTextAlign::Right);
    }

    #[test]
    fn climber_supports_end_text_align_keyword() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context);
        let parent = root.create_element(DomTag::Div).expect("parent");
        let child = root.create_element(DomTag::Div).expect("child");
        root.append_child(root.root, parent).expect("append parent");
        root.append_child(parent, child).expect("append child");
        root.set_attr(
            parent,
            DomAttribute::TextAlign,
            Some(DomAttributeValue::String("end".to_string())),
        )
        .expect("parent text-align");

        let inherited = inherited_style_context_from_ancestors(&root, child).expect("climb");

        assert_eq!(inherited.effective_text_align(), DomTextAlign::End);
    }

    #[test]
    fn climber_supports_direction_property_keyword() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context);
        let parent = root.create_element(DomTag::Div).expect("parent");
        let child = root.create_element(DomTag::Div).expect("child");
        root.append_child(root.root, parent).expect("append parent");
        root.append_child(parent, child).expect("append child");
        root.set_attr(
            parent,
            DomAttribute::Direction,
            Some(DomAttributeValue::String("rtl".to_string())),
        )
        .expect("parent direction");

        let inherited = inherited_style_context_from_ancestors(&root, child).expect("climb");

        assert_eq!(inherited.effective_text_direction(), DomTextDirection::Rtl);
    }

    #[test]
    fn climber_prefers_direction_property_over_dir_attribute() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context);
        let parent = root.create_element(DomTag::Div).expect("parent");
        let child = root.create_element(DomTag::Div).expect("child");
        root.append_child(root.root, parent).expect("append parent");
        root.append_child(parent, child).expect("append child");
        root.set_attr(
            parent,
            DomAttribute::Dir,
            Some(DomAttributeValue::String("ltr".to_string())),
        )
        .expect("parent dir");
        root.set_attr(
            parent,
            DomAttribute::Direction,
            Some(DomAttributeValue::String("rtl".to_string())),
        )
        .expect("parent direction");

        let inherited = inherited_style_context_from_ancestors(&root, child).expect("climb");

        assert_eq!(inherited.effective_text_direction(), DomTextDirection::Rtl);
    }
}
