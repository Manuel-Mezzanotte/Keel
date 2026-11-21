//! Vertical rulers at the columns `editor.rulers` names, as VS Code draws
//! them: a thin line where a line of that many characters would end.
//! Each ruler is a number, or `{ "column": 100, "color": "#ff000060" }`.

use gpui::{AnyElement, Bounds, Hsla, canvas, point};
use gpui_component::{ActiveTheme as _, RopeExt as _};

use super::*;

/// The rulers `value` names: their columns and the colors given.
pub(super) fn rulers_of(value: Option<&Value>) -> Vec<(usize, Option<Hsla>)> {
    let Some(items) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| match item {
            Value::Number(column) => Some((column.as_u64()? as usize, None)),
            Value::Object(ruler) => Some((
                ruler.get("column")?.as_u64()? as usize,
                ruler
                    .get("color")
                    .and_then(Value::as_str)
                    .and_then(|color| gpui::Rgba::try_from(color).ok())
                    .map(Into::into),
            )),
            _ => None,
        })
        .filter(|(column, _)| (1..1000).contains(column))
        .collect()
}

impl Workbench {
    /// The rulers over pane `index`'s editor, when the settings name some.
    pub(super) fn render_rulers(&self, index: usize, cx: &App) -> Option<AnyElement> {
        let pane = self.panes.get(index)?;
        let language = self.documents.get(&pane.uri)?.language_id.clone();
        let rulers = rulers_of(
            self.language_setting(&language, "editor.rulers")
                .as_ref()
                .or(self.settings.get("editor.rulers")),
        );
        if rulers.is_empty() {
            return None;
        }
        let editor = pane.editor.read(cx);
        let rows = editor.visible_row_range()?;
        let text = editor.text();
        // Where the text starts, and how wide a character is, from a line
        // on the screen that starts with a plain one.
        let mut start = None;
        let mut width = None;
        for row in rows {
            let offset = text.line_start_offset(row);
            let Some(line_start) = editor.range_to_bounds(&(offset..offset)) else {
                continue;
            };
            start.get_or_insert(line_start.origin.x);
            let first = text.slice(offset..text.len().min(offset + 1)).to_string();
            if first.len() == 1
                && first != "\t"
                && first != "\n"
                && let Some(char) = editor.range_to_bounds(&(offset..offset + 1))
            {
                start = Some(line_start.origin.x);
                width = Some(char.size.width);
                break;
            }
        }
        let (start, width) = (start?, width?);
        // The line numbers end where the text starts when not scrolled
        // sideways: no ruler over them.
        let scrolled = editor.text_bounds()?.origin.x - editor.input_bounds().origin.x;
        let text_left = start - scrolled;
        let color = cx.theme().border;
        Some(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    for (column, own) in &rulers {
                        let x = start + width * *column as f32;
                        if x <= bounds.left().max(text_left) || x >= bounds.right() {
                            continue;
                        }
                        window.paint_quad(gpui::fill(
                            Bounds::new(
                                point(x, bounds.top()),
                                gpui::size(px(1.), bounds.size.height),
                            ),
                            own.unwrap_or(color),
                        ));
                    }
                },
            )
            .absolute()
            .inset_0()
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rulers_as_numbers_or_with_their_colors() {
        let value = serde_json::json!([80, { "column": 100, "color": "#ff0000" }, 0, "x"]);
        let rulers = rulers_of(Some(&value));
        assert_eq!(rulers.len(), 2);
        assert_eq!(rulers[0], (80, None));
        assert_eq!(rulers[1].0, 100);
        assert!(rulers[1].1.is_some());
        assert!(rulers_of(None).is_empty());
    }
}
