use std::collections::BTreeMap;

pub const VIEWPORT_SIZE_PX: (u32, u32) = (72, 72);
pub const MARGIN_PX: i32 = 2;
pub const BORDER_PX: i32 = 1;
pub const LABELS: [&str; 9] = ["a", "b", "c", "d", "e", "f", "g", "h", "i"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RectPx {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OffsetPx {
    pub left: i32,
    pub top: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(clippy::struct_field_names)]
pub struct GeometryModel {
    pub margin_px: i32,
    pub border_px: i32,
    pub root_border_box_width_px: i32,
    pub root_border_box_height_px: i32,
}

impl Default for GeometryModel {
    fn default() -> Self {
        Self {
            margin_px: MARGIN_PX,
            border_px: BORDER_PX,
            root_border_box_width_px: 56,
            root_border_box_height_px: 56,
        }
    }
}

impl GeometryModel {
    fn nesting_step_px(self) -> i32 {
        self.margin_px + self.border_px
    }

    fn size_step_px(self) -> i32 {
        self.nesting_step_px() * 2
    }

    #[allow(clippy::unused_self)]
    pub fn label_index(self, label: &str) -> usize {
        LABELS
            .iter()
            .position(|candidate| *candidate == label)
            .expect("label should resolve to a stable depth")
    }

    #[allow(clippy::unused_self)]
    pub fn owning_layer_label(self, label: &str) -> &'static str {
        match label {
            "a" | "b" | "c" => "a",
            "d" | "e" | "f" => "d",
            "g" | "h" | "i" => "g",
            other => panic!("unexpected label {other}"),
        }
    }

    pub fn content_width_px(self, depth: usize) -> i32 {
        self.border_box_width_px(depth) - (self.border_px * 2)
    }

    pub fn content_height_px(self, depth: usize) -> i32 {
        self.border_box_height_px(depth) - (self.border_px * 2)
    }

    pub fn border_box_width_px(self, depth: usize) -> i32 {
        self.root_border_box_width_px - (i32_from_usize_size(depth) * self.size_step_px())
    }

    pub fn border_box_height_px(self, depth: usize) -> i32 {
        self.root_border_box_height_px - (i32_from_usize_size(depth) * self.size_step_px())
    }

    pub fn local_rect(self, label: &str) -> RectPx {
        let depth = self.label_index(label);
        let layer_depth = self.label_index(self.owning_layer_label(label));
        let relative_depth = i32_from_usize_size(depth) - i32_from_usize_size(layer_depth);
        let inset = relative_depth * self.nesting_step_px();
        RectPx {
            left: inset,
            top: inset,
            width: self.border_box_width_px(depth),
            height: self.border_box_height_px(depth),
        }
    }

    pub fn local_rects(self) -> BTreeMap<&'static str, RectPx> {
        LABELS
            .iter()
            .map(|label| (*label, self.local_rect(label)))
            .collect()
    }

    pub fn global_rect(self, label: &str, offsets: &BTreeMap<&'static str, OffsetPx>) -> RectPx {
        let depth = self.label_index(label);
        let inset = self.margin_px + (i32_from_usize_size(depth) * self.nesting_step_px());
        let accumulated_offset = LABELS[..=depth]
            .iter()
            .map(|current| offsets.get(current).copied().unwrap_or_default())
            .fold(OffsetPx::default(), |acc, current| OffsetPx {
                left: acc.left + current.left,
                top: acc.top + current.top,
            });
        RectPx {
            left: inset + accumulated_offset.left,
            top: inset + accumulated_offset.top,
            width: self.border_box_width_px(depth),
            height: self.border_box_height_px(depth),
        }
    }

    pub fn global_rects(
        self,
        offsets: &BTreeMap<&'static str, OffsetPx>,
    ) -> BTreeMap<&'static str, RectPx> {
        LABELS
            .iter()
            .map(|label| (*label, self.global_rect(label, offsets)))
            .collect()
    }
}

fn i32_from_usize_size(value: usize) -> i32 {
    i32::try_from(value).expect("fixture depth should fit i32")
}
