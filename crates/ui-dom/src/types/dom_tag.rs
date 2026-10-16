//! Built-in DOM element tags and their default behavior.
//!
//! This file represents the authored element vocabulary supported by `ui-dom`.
//! Its purpose is to centralize default attributes and behavior flags for
//! built-in tags so callers can create elements without re-deriving those
//! defaults in each app.
//!
//! Invariants and rules:
//!
//! - built-in tag behavior must stay deterministic and crate-owned
//! - app-local styling or scene policy must not leak into descriptors here
//! - defaults should remain strict and unsurprising
//!
//! Advice and code philosophy:
//!
//! - keep tag descriptors literal and low ceremony
//! - if a behavior is not shared across apps, do not teach it to this file
use smallvec::{SmallVec, smallvec};

pub use crate::types::dom_after_attr_write_intrinsic_update::DomAfterAttrWriteIntrinsicUpdate;

use crate::types::dom_element_descriptor_builder::DomElementDescriptorBuilder;
use crate::types::element_behaviors::{
    DomElementAfterClick, DomElementAfterFocus, DomElementIntrinsicKind, DomElementOnActivation,
    DomElementOnBlur, DomElementOnKeyboard, DomElementOnPointer, DomElementPresentOptionsBehavior,
};
use crate::types::{DomAttribute, DomAttributeValue, DomAttrs};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DomTag {
    Div,
    Img,
    Span,
    Ul,
    Li,
    Button,
    Input,
    Select,
    Datalist,
    Option,
    Textarea,
    Custom(String),
}

/// Declares whether a built-in element participates in text editing.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DomElementEditKind {
    #[default]
    None,
    SingleLine,
    MultiLine,
}

/// Declares whether one lifecycle should enter the element-control path.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DomElementLifecycleBehavior {
    #[default]
    None,
    ElementControls,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DomElementBehavior {
    pub focusable: bool,
    pub active_pseudo: bool,
    /// Canonical edit capability for this element descriptor.
    pub edit_kind: DomElementEditKind,
    pub intrinsic_kind: DomElementIntrinsicKind,
    pub after_attr_write_intrinsic_update: DomAfterAttrWriteIntrinsicUpdate,
    pub after_style_recompute_intrinsic_update_behavior: DomElementLifecycleBehavior,
    pub on_keyboard: DomElementOnKeyboard,
    pub on_pointer: DomElementOnPointer,
    pub after_click: DomElementAfterClick,
    pub on_activation: DomElementOnActivation,
    pub after_focus: DomElementAfterFocus,
    pub on_blur: DomElementOnBlur,
    pub hit_test_behavior: DomElementLifecycleBehavior,
    pub present_options_behavior: DomElementPresentOptionsBehavior,
}

pub type DomStyleDeclarationList = SmallVec<[(DomAttribute, DomAttributeValue); 4]>;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DomElementStateStyleDeclarations {
    pub hover: DomStyleDeclarationList,
    pub active: DomStyleDeclarationList,
    pub focus: DomStyleDeclarationList,
    pub focus_visible: DomStyleDeclarationList,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DomElementDescriptor {
    pub default_attrs: DomAttrs,
    pub behavior: DomElementBehavior,
    pub state_style_declarations: DomElementStateStyleDeclarations,
}

fn with_default_field_state_styles(
    builder: DomElementDescriptorBuilder,
) -> DomElementDescriptorBuilder {
    builder
        .hover_style(smallvec![(
            DomAttribute::Custom("background-color".to_string()),
            color_value("#191d26"),
        )])
        .active_style(smallvec![(
            DomAttribute::Custom("background-color".to_string()),
            color_value("#161a22"),
        )])
        .focus_style(smallvec![
            (
                DomAttribute::Custom("outline-color".to_string()),
                color_value("#7aa2ff"),
            ),
            (
                DomAttribute::Custom("outline-width".to_string()),
                DomAttributeValue::String("2px".to_string()),
            ),
            (
                DomAttribute::Custom("outline-offset".to_string()),
                DomAttributeValue::String("0px".to_string()),
            ),
        ])
        .focus_visible_style(smallvec![
            (
                DomAttribute::Custom("outline-color".to_string()),
                color_value("#7aa2ff"),
            ),
            (
                DomAttribute::Custom("outline-width".to_string()),
                DomAttributeValue::String("2px".to_string()),
            ),
            (
                DomAttribute::Custom("outline-offset".to_string()),
                DomAttributeValue::String("0px".to_string()),
            ),
        ])
}

fn color_value(value: &str) -> DomAttributeValue {
    DomAttributeValue::Color(
        crate::types::dom_attr_parse::parse_css_color_rgba(value)
            .expect("element descriptor color should parse"),
    )
}

impl DomTag {
    #[must_use]
    pub(crate) fn selector_name(&self) -> &str {
        match self {
            Self::Div => "div",
            Self::Img => "img",
            Self::Span => "span",
            Self::Ul => "ul",
            Self::Li => "li",
            Self::Button => "button",
            Self::Input => "input",
            Self::Select => "select",
            Self::Datalist => "datalist",
            Self::Option => "option",
            Self::Textarea => "textarea",
            Self::Custom(tag) => tag.as_str(),
        }
    }

    /// Returns the descriptor that seeds default attrs and built-in behavior
    /// for this element tag.
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn descriptor(&self) -> DomElementDescriptor {
        match self {
            Self::Div | Self::Ul | Self::Li => {
                DomElementDescriptorBuilder::new().display("block").build()
            }
            Self::Img => DomElementDescriptorBuilder::new()
                .display("inline-block")
                .behavior(DomElementBehavior {
                    focusable: false,
                    active_pseudo: false,
                    edit_kind: DomElementEditKind::None,
                    intrinsic_kind: DomElementIntrinsicKind::Img,
                    after_attr_write_intrinsic_update:
                        DomAfterAttrWriteIntrinsicUpdate::AttributeList(smallvec![
                            DomAttribute::Src,
                            DomAttribute::Width,
                            DomAttribute::Height,
                        ]),
                    after_style_recompute_intrinsic_update_behavior:
                        DomElementLifecycleBehavior::None,
                    on_keyboard: DomElementOnKeyboard::None,
                    on_pointer: DomElementOnPointer::None,
                    after_click: DomElementAfterClick::None,
                    on_activation: DomElementOnActivation::None,
                    after_focus: DomElementAfterFocus::None,
                    on_blur: DomElementOnBlur::None,
                    hit_test_behavior: DomElementLifecycleBehavior::None,
                    present_options_behavior: DomElementPresentOptionsBehavior::None,
                })
                .build(),
            Self::Span | Self::Custom(_) => {
                DomElementDescriptorBuilder::new().display("inline").build()
            }
            Self::Button => DomElementDescriptorBuilder::new()
                .display("inline-block")
                .behavior(DomElementBehavior {
                    focusable: true,
                    active_pseudo: true,
                    edit_kind: DomElementEditKind::None,
                    intrinsic_kind: DomElementIntrinsicKind::None,
                    after_attr_write_intrinsic_update: DomAfterAttrWriteIntrinsicUpdate::None,
                    after_style_recompute_intrinsic_update_behavior:
                        DomElementLifecycleBehavior::None,
                    on_keyboard: DomElementOnKeyboard::Button,
                    on_pointer: DomElementOnPointer::None,
                    after_click: DomElementAfterClick::None,
                    on_activation: DomElementOnActivation::None,
                    after_focus: DomElementAfterFocus::None,
                    on_blur: DomElementOnBlur::None,
                    hit_test_behavior: DomElementLifecycleBehavior::None,
                    present_options_behavior: DomElementPresentOptionsBehavior::None,
                })
                .hover_style(smallvec![(
                    DomAttribute::Custom("background-color".to_string()),
                    color_value("#242833"),
                ),])
                .active_style(smallvec![(
                    DomAttribute::Custom("background-color".to_string()),
                    color_value("#161a22"),
                ),])
                .focus_visible_style(smallvec![
                    (
                        DomAttribute::Custom("outline-color".to_string()),
                        color_value("#7aa2ff"),
                    ),
                    (
                        DomAttribute::Custom("outline-width".to_string()),
                        DomAttributeValue::String("2px".to_string()),
                    ),
                    (
                        DomAttribute::Custom("outline-offset".to_string()),
                        DomAttributeValue::String("2px".to_string()),
                    ),
                ])
                .attr(
                    DomAttribute::Type,
                    DomAttributeValue::String("button".to_string()),
                )
                .build(),
            Self::Input => with_default_field_state_styles(
                DomElementDescriptorBuilder::new()
                    .display("inline-block")
                    .behavior(DomElementBehavior {
                        focusable: true,
                        active_pseudo: true,
                        edit_kind: DomElementEditKind::SingleLine,
                        intrinsic_kind: DomElementIntrinsicKind::ElementDefaultPreferredSize,
                        after_attr_write_intrinsic_update:
                            DomAfterAttrWriteIntrinsicUpdate::AttributeList(smallvec![
                                DomAttribute::Value,
                                DomAttribute::Placeholder,
                                DomAttribute::LineHeight,
                                DomAttribute::Type,
                                DomAttribute::Checked,
                                DomAttribute::Min,
                                DomAttribute::Max,
                                DomAttribute::Step,
                            ]),
                        after_style_recompute_intrinsic_update_behavior:
                            DomElementLifecycleBehavior::ElementControls,
                        on_keyboard: DomElementOnKeyboard::InputControl,
                        on_pointer: DomElementOnPointer::InputControl,
                        after_click: DomElementAfterClick::Activate,
                        on_activation: DomElementOnActivation::Toggle,
                        after_focus: DomElementAfterFocus::PresentedOptions,
                        on_blur: DomElementOnBlur::PresentedOptions,
                        hit_test_behavior: DomElementLifecycleBehavior::ElementControls,
                        present_options_behavior: DomElementPresentOptionsBehavior::TextInput,
                    }),
            )
            .attr(
                DomAttribute::Type,
                DomAttributeValue::String("text".to_string()),
            )
            .attr(
                DomAttribute::Value,
                DomAttributeValue::String(String::new()),
            )
            .build(),
            Self::Select => with_default_field_state_styles(
                DomElementDescriptorBuilder::new()
                    .display("inline-block")
                    .behavior(DomElementBehavior {
                        focusable: true,
                        active_pseudo: false,
                        edit_kind: DomElementEditKind::None,
                        intrinsic_kind: DomElementIntrinsicKind::ElementDefaultPreferredSize,
                        after_attr_write_intrinsic_update:
                            DomAfterAttrWriteIntrinsicUpdate::AttributeList(smallvec![
                                DomAttribute::Value,
                                DomAttribute::LineHeight,
                            ]),
                        after_style_recompute_intrinsic_update_behavior:
                            DomElementLifecycleBehavior::ElementControls,
                        on_keyboard: DomElementOnKeyboard::Select,
                        on_pointer: DomElementOnPointer::None,
                        after_click: DomElementAfterClick::Activate,
                        on_activation: DomElementOnActivation::Select,
                        after_focus: DomElementAfterFocus::None,
                        on_blur: DomElementOnBlur::Select,
                        hit_test_behavior: DomElementLifecycleBehavior::None,
                        present_options_behavior: DomElementPresentOptionsBehavior::None,
                    }),
            )
            .attr(
                DomAttribute::Value,
                DomAttributeValue::String(String::new()),
            )
            .build(),
            Self::Datalist => DomElementDescriptorBuilder::new().display("none").build(),
            Self::Option => DomElementDescriptorBuilder::new()
                .display("none")
                .behavior(DomElementBehavior {
                    focusable: false,
                    active_pseudo: false,
                    edit_kind: DomElementEditKind::None,
                    intrinsic_kind: DomElementIntrinsicKind::None,
                    after_attr_write_intrinsic_update: DomAfterAttrWriteIntrinsicUpdate::None,
                    after_style_recompute_intrinsic_update_behavior:
                        DomElementLifecycleBehavior::None,
                    on_keyboard: DomElementOnKeyboard::None,
                    on_pointer: DomElementOnPointer::None,
                    after_click: DomElementAfterClick::None,
                    on_activation: DomElementOnActivation::None,
                    after_focus: DomElementAfterFocus::None,
                    on_blur: DomElementOnBlur::None,
                    hit_test_behavior: DomElementLifecycleBehavior::None,
                    present_options_behavior: DomElementPresentOptionsBehavior::None,
                })
                .build(),
            Self::Textarea => with_default_field_state_styles(
                DomElementDescriptorBuilder::new()
                    .display("inline-block")
                    .behavior(DomElementBehavior {
                        focusable: true,
                        active_pseudo: false,
                        edit_kind: DomElementEditKind::MultiLine,
                        intrinsic_kind: DomElementIntrinsicKind::ElementDefaultPreferredSize,
                        after_attr_write_intrinsic_update:
                            DomAfterAttrWriteIntrinsicUpdate::AttributeList(smallvec![
                                DomAttribute::Value,
                                DomAttribute::Placeholder,
                                DomAttribute::LineHeight,
                            ]),
                        after_style_recompute_intrinsic_update_behavior:
                            DomElementLifecycleBehavior::ElementControls,
                        on_keyboard: DomElementOnKeyboard::TextEdit,
                        on_pointer: DomElementOnPointer::TextEdit,
                        after_click: DomElementAfterClick::None,
                        on_activation: DomElementOnActivation::None,
                        after_focus: DomElementAfterFocus::None,
                        on_blur: DomElementOnBlur::None,
                        hit_test_behavior: DomElementLifecycleBehavior::ElementControls,
                        present_options_behavior: DomElementPresentOptionsBehavior::None,
                    }),
            )
            .attr(
                DomAttribute::Value,
                DomAttributeValue::String(String::new()),
            )
            .build(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smallvec::smallvec;

    #[test]
    fn button_descriptor_sets_button_defaults() {
        let descriptor = DomTag::Button.descriptor();

        assert_eq!(
            descriptor
                .default_attrs
                .attributes
                .get(&DomAttribute::Display),
            Some(&DomAttributeValue::String("inline-block".to_string()))
        );
        assert_eq!(
            descriptor.default_attrs.attributes.get(&DomAttribute::Type),
            Some(&DomAttributeValue::String("button".to_string()))
        );
        assert_eq!(
            descriptor.behavior,
            DomElementBehavior {
                focusable: true,
                active_pseudo: true,
                edit_kind: DomElementEditKind::None,
                intrinsic_kind: DomElementIntrinsicKind::None,
                after_attr_write_intrinsic_update: DomAfterAttrWriteIntrinsicUpdate::None,
                after_style_recompute_intrinsic_update_behavior: DomElementLifecycleBehavior::None,
                on_keyboard: DomElementOnKeyboard::Button,
                on_pointer: DomElementOnPointer::None,
                after_click: DomElementAfterClick::None,
                on_activation: DomElementOnActivation::None,
                after_focus: DomElementAfterFocus::None,
                on_blur: DomElementOnBlur::None,
                hit_test_behavior: DomElementLifecycleBehavior::None,
                present_options_behavior: DomElementPresentOptionsBehavior::None,
            }
        );
        assert_eq!(
            descriptor
                .state_style_declarations
                .focus_visible
                .iter()
                .find(|(attribute, _)| {
                    *attribute == DomAttribute::Custom("outline-color".to_string())
                }),
            Some(&(
                DomAttribute::Custom("outline-color".to_string()),
                color_value("#7aa2ff"),
            ))
        );
    }

    #[test]
    fn textarea_descriptor_sets_multiline_behavior() {
        let descriptor = DomTag::Textarea.descriptor();

        assert_eq!(
            descriptor
                .default_attrs
                .attributes
                .get(&DomAttribute::Value),
            Some(&DomAttributeValue::String(String::new()))
        );
        assert_eq!(
            descriptor.behavior,
            DomElementBehavior {
                focusable: true,
                active_pseudo: false,
                edit_kind: DomElementEditKind::MultiLine,
                intrinsic_kind: DomElementIntrinsicKind::ElementDefaultPreferredSize,
                after_attr_write_intrinsic_update: DomAfterAttrWriteIntrinsicUpdate::AttributeList(
                    smallvec![
                        DomAttribute::Value,
                        DomAttribute::Placeholder,
                        DomAttribute::LineHeight,
                    ]
                ),
                after_style_recompute_intrinsic_update_behavior:
                    DomElementLifecycleBehavior::ElementControls,
                on_keyboard: DomElementOnKeyboard::TextEdit,
                on_pointer: DomElementOnPointer::TextEdit,
                after_click: DomElementAfterClick::None,
                on_activation: DomElementOnActivation::None,
                after_focus: DomElementAfterFocus::None,
                on_blur: DomElementOnBlur::None,
                hit_test_behavior: DomElementLifecycleBehavior::ElementControls,
                present_options_behavior: DomElementPresentOptionsBehavior::None,
            }
        );
        assert!(!descriptor.state_style_declarations.focus.is_empty());
        assert!(!descriptor.state_style_declarations.hover.is_empty());
        assert!(!descriptor.state_style_declarations.active.is_empty());
        assert!(!descriptor.state_style_declarations.focus_visible.is_empty());
    }

    #[test]
    fn input_descriptor_sets_text_edit_kind() {
        let descriptor = DomTag::Input.descriptor();

        assert_eq!(
            descriptor.behavior,
            DomElementBehavior {
                focusable: true,
                active_pseudo: true,
                edit_kind: DomElementEditKind::SingleLine,
                intrinsic_kind: DomElementIntrinsicKind::ElementDefaultPreferredSize,
                after_attr_write_intrinsic_update: DomAfterAttrWriteIntrinsicUpdate::AttributeList(
                    smallvec![
                        DomAttribute::Value,
                        DomAttribute::Placeholder,
                        DomAttribute::LineHeight,
                        DomAttribute::Type,
                        DomAttribute::Checked,
                        DomAttribute::Min,
                        DomAttribute::Max,
                        DomAttribute::Step,
                    ]
                ),
                after_style_recompute_intrinsic_update_behavior:
                    DomElementLifecycleBehavior::ElementControls,
                on_keyboard: DomElementOnKeyboard::InputControl,
                on_pointer: DomElementOnPointer::InputControl,
                after_click: DomElementAfterClick::Activate,
                on_activation: DomElementOnActivation::Toggle,
                after_focus: DomElementAfterFocus::PresentedOptions,
                on_blur: DomElementOnBlur::PresentedOptions,
                hit_test_behavior: DomElementLifecycleBehavior::ElementControls,
                present_options_behavior: DomElementPresentOptionsBehavior::TextInput,
            }
        );
        assert!(
            descriptor
                .behavior
                .after_attr_write_intrinsic_update
                .matches(&DomAttribute::Value)
        );
        assert!(
            descriptor
                .behavior
                .after_attr_write_intrinsic_update
                .matches(&DomAttribute::LineHeight)
        );
        assert!(!descriptor.state_style_declarations.hover.is_empty());
        assert!(!descriptor.state_style_declarations.active.is_empty());
        assert!(!descriptor.state_style_declarations.focus.is_empty());
        assert!(!descriptor.state_style_declarations.focus_visible.is_empty());
    }

    #[test]
    fn ul_descriptor_sets_block_display_and_inert_behavior() {
        let descriptor = DomTag::Ul.descriptor();

        assert_eq!(
            descriptor
                .default_attrs
                .attributes
                .get(&DomAttribute::Display),
            Some(&DomAttributeValue::String("block".to_string()))
        );
        assert_eq!(descriptor.behavior, DomElementBehavior::default());
    }

    #[test]
    fn span_descriptor_sets_inline_display_and_inert_behavior() {
        let descriptor = DomTag::Span.descriptor();

        assert_eq!(
            descriptor
                .default_attrs
                .attributes
                .get(&DomAttribute::Display),
            Some(&DomAttributeValue::String("inline".to_string()))
        );
        assert_eq!(descriptor.behavior, DomElementBehavior::default());
    }

    #[test]
    fn img_descriptor_marks_intrinsic_update_attrs() {
        let descriptor = DomTag::Img.descriptor();

        assert_eq!(
            descriptor.behavior.intrinsic_kind,
            DomElementIntrinsicKind::Img
        );
        assert!(
            descriptor
                .behavior
                .after_attr_write_intrinsic_update
                .matches(&DomAttribute::Src)
        );
        assert!(
            descriptor
                .behavior
                .after_attr_write_intrinsic_update
                .matches(&DomAttribute::Width)
        );
        assert!(
            descriptor
                .behavior
                .after_attr_write_intrinsic_update
                .matches(&DomAttribute::Height)
        );
        assert!(
            !descriptor
                .behavior
                .after_attr_write_intrinsic_update
                .matches(&DomAttribute::FontSize)
        );
    }

    #[test]
    fn li_descriptor_sets_block_display_and_inert_behavior() {
        let descriptor = DomTag::Li.descriptor();

        assert_eq!(
            descriptor
                .default_attrs
                .attributes
                .get(&DomAttribute::Display),
            Some(&DomAttributeValue::String("block".to_string()))
        );
        assert_eq!(descriptor.behavior, DomElementBehavior::default());
    }

    #[test]
    fn select_descriptor_sets_select_lifecycle_behavior() {
        let descriptor = DomTag::Select.descriptor();

        assert_eq!(
            descriptor.behavior,
            DomElementBehavior {
                focusable: true,
                active_pseudo: false,
                edit_kind: DomElementEditKind::None,
                intrinsic_kind: DomElementIntrinsicKind::ElementDefaultPreferredSize,
                after_attr_write_intrinsic_update: DomAfterAttrWriteIntrinsicUpdate::AttributeList(
                    smallvec![DomAttribute::Value, DomAttribute::LineHeight,]
                ),
                after_style_recompute_intrinsic_update_behavior:
                    DomElementLifecycleBehavior::ElementControls,
                on_keyboard: DomElementOnKeyboard::Select,
                on_pointer: DomElementOnPointer::None,
                after_click: DomElementAfterClick::Activate,
                on_activation: DomElementOnActivation::Select,
                after_focus: DomElementAfterFocus::None,
                on_blur: DomElementOnBlur::Select,
                hit_test_behavior: DomElementLifecycleBehavior::None,
                present_options_behavior: DomElementPresentOptionsBehavior::None,
            }
        );
        assert_eq!(
            descriptor
                .default_attrs
                .attributes
                .get(&DomAttribute::Display),
            Some(&DomAttributeValue::String("inline-block".to_string()))
        );
        assert_eq!(
            descriptor
                .default_attrs
                .attributes
                .get(&DomAttribute::Value),
            Some(&DomAttributeValue::String(String::new()))
        );
    }

    #[test]
    fn datalist_and_option_descriptors_default_to_non_rendering() {
        let datalist = DomTag::Datalist.descriptor();
        let option = DomTag::Option.descriptor();

        assert_eq!(
            datalist
                .default_attrs
                .attributes
                .get(&DomAttribute::Display),
            Some(&DomAttributeValue::String("none".to_string()))
        );
        assert_eq!(
            option.default_attrs.attributes.get(&DomAttribute::Display),
            Some(&DomAttributeValue::String("none".to_string()))
        );
        assert_eq!(option.behavior.after_click, DomElementAfterClick::None);
        assert_eq!(option.behavior.on_activation, DomElementOnActivation::None);
    }
}
